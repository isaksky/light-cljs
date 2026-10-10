use light_cljs::interpreter::{
    Arity, Context, ErrorClass, EvalErrorKind as E, EvalPhase, FileSourceProvider, LoadError,
    LoadRequest, LoadState, Name, NativeDefinition, NativeEffects, NativeMetadata, Reload,
    ResourceLimits, RootedValue, SourceInput, SourceProvider, ValueView,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
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

fn sources() -> HashMap<Name, SourceInput> {
    include_str!("reference/sci/load-sources.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let parts: Vec<_> = line.split('\t').collect();
            (
                Name::new(None, parts[0]),
                SourceInput::new(parts[1], parts[2]),
            )
        })
        .collect()
}
fn setup() -> Context {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    TRACE.with(|trace| trace.borrow_mut().clear());
    let sources = sources();
    c.set_source_provider(move |request: &LoadRequest| {
        Ok(sources.get(&request.namespace).cloned())
    });
    c
}
#[test]
fn loading_matches_pinned_sci_values_effects_and_order() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases
        .iter()
        .filter(|case| case.id.starts_with("sci-loading-"))
    {
        let mut c = setup();
        let outcome = match c.eval_string(case.source) {
            Ok(value) => protocol::Outcome::Returned(wire(&value)),
            Err(error) => {
                assert_eq!(error.phase, EvalPhase::Analysis, "{}: {error}", case.id);
                assert!(
                    matches!(error.kind, E::UnresolvedSymbol(_)),
                    "{}: {error}",
                    case.id
                );
                protocol::Outcome::Error {
                    phase: protocol::Phase::Analysis,
                    category: protocol::Category::UnresolvedSymbol,
                }
            }
        };
        assert_eq!(outcome, case.outcome, "{}", case.id);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 51);
}

fn eval(c: &mut Context, text: &str) -> ValueView {
    c.eval_string(text).unwrap().inspect()
}

#[test]
fn default_context_has_no_source_search_and_alias_only_does_not_load() {
    let mut c = Context::new();
    let name = Name::new(None, "anything");
    assert_eq!(c.namespace_load_state(&name), LoadState::Unloaded);
    let e = c.eval_string("(require 'anything)").unwrap_err();
    assert!(matches!(e.kind, E::Load(LoadError::MissingNamespace(_))));
    assert_eq!(c.namespace_load_state(&name), LoadState::Failed);
    c.set_source_provider(|_: &LoadRequest| panic!("as-alias must not call provider"));
    c.eval_string("(require '[absent :as-alias a]) ::a/k")
        .unwrap();
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "absent")),
        LoadState::Unloaded
    );
    c.clear_source_provider();
    c.eval_string("(require 'clojure.core :reload-all)")
        .unwrap();
    c.collect_garbage();
    assert_eq!(eval(&mut c, "(+ 1 2)"), ValueView::Integer(3));
}

#[test]
fn requests_states_cycles_and_normal_cache_are_observable_and_deterministic() {
    let mut c = setup();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let captured = requests.clone();
    let sources = sources();
    c.set_source_provider(move |r: &LoadRequest| {
        captured.borrow_mut().push(r.clone());
        Ok(sources.get(&r.namespace).cloned())
    });
    let source = c.source(
        "entry.clj",
        "(require 'load.alpha 'load.beta) (require 'load.alpha)",
    );
    c.eval_source(&source).unwrap();
    assert_eq!(
        requests
            .borrow()
            .iter()
            .map(|r| r.namespace.clone())
            .collect::<Vec<_>>(),
        ["load.alpha", "load.shared", "load.beta"].map(|n| Name::new(None, n))
    );
    assert_eq!(
        requests.borrow()[0].requesting_file.as_deref(),
        Some("entry.clj")
    );
    assert_eq!(
        requests.borrow()[1].requesting_file.as_deref(),
        Some("load/alpha.clj")
    );
    assert_eq!(
        requests.borrow()[1].requesting_namespace,
        Name::new(None, "load.alpha")
    );
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "load.shared")),
        LoadState::Loaded
    );
    let e = c.eval_string("(require 'load.cycle-a)").unwrap_err();
    assert!(
        matches!(e.kind, E::Load(LoadError::Cycle(ref path)) if path == &["load.cycle-a", "load.cycle-b", "load.cycle-a"].map(|n| Name::new(None, n)))
    );
    for name in ["load.cycle-a", "load.cycle-b"] {
        assert_eq!(
            c.namespace_load_state(&Name::new(None, name)),
            LoadState::Failed
        );
    }
    c.eval_string("(require 'load.alpha :reload-all)").unwrap();
    let requests = requests.borrow();
    assert_eq!(requests[requests.len() - 2].reload, Reload::All);
    assert_eq!(requests[requests.len() - 1].reload, Reload::All);
}

#[test]
fn failed_loads_retry_without_stale_aliases_or_current_namespace_ids() {
    let source = Rc::new(RefCell::new("(ns retry) (def x 1)".to_owned()));
    let captured = source.clone();
    let mut c = Context::new();
    c.set_source_provider(move |_: &LoadRequest| {
        Ok(Some(SourceInput::new(
            "retry.clj",
            captured.borrow().clone(),
        )))
    });
    c.eval_string("(require '[retry :as r])").unwrap();
    *source.borrow_mut() = "(ns retry) (def stale-definition 2) (/ 1 0)".into();
    assert!(c.eval_string("(require 'retry :reload)").is_err());
    assert!(c.find_namespace(&Name::new(None, "retry")).is_none());
    *source.borrow_mut() = "(ns retry) (def x 3)".into();
    assert_eq!(eval(&mut c, "(require 'retry) r/x"), ValueView::Integer(3));
    assert_eq!(
        eval(&mut c, "(require '[retry :as r]) r/x"),
        ValueView::Integer(3)
    );
    c.eval_string("(in-ns 'retry)").unwrap();
    *source.borrow_mut() = "(ns retry) (/ 1 0)".into();
    assert!(c.eval_string("(require 'retry :reload)").is_err());
    c.collect_garbage();
    assert_eq!(eval(&mut c, "x"), ValueView::Integer(3));
    *source.borrow_mut() = "(ns retry) (def x 4)".into();
    assert_eq!(eval(&mut c, "(require 'retry) x"), ValueView::Integer(4));
    assert_eq!(
        eval(&mut c, "(nil? (resolve 'stale-definition))"),
        ValueView::Boolean(true)
    );
}

#[test]
fn reload_updates_var_roots_and_future_expansions_without_replaying_retained_code() {
    let source = Rc::new(RefCell::new(
        "(ns changing) (def x 1) (def obsolete 9) (defmacro m [] 10)".to_owned(),
    ));
    let captured = source.clone();
    let mut c = Context::new();
    c.set_source_provider(move |_: &LoadRequest| {
        Ok(Some(SourceInput::new(
            "changing.clj",
            captured.borrow().clone(),
        )))
    });
    c.eval_string("(require '[changing :as a :refer [x m]])")
        .unwrap();
    let old_var = c.eval_string("#'a/x").unwrap();
    let input = c.source("consumer.clj", "[x (m)]");
    let form = c.reader(&input).unwrap().read_form().unwrap().unwrap();
    let analyzed = c.analyze_form(&form).unwrap();
    *source.borrow_mut() = "(ns changing) (def x 2) (defmacro m [] 20)".into();
    c.eval_string("(require 'changing :reload)").unwrap();
    assert_eq!(old_var.var_id(), c.eval_string("#'a/x").unwrap().var_id());
    assert_eq!(
        wire(&c.eval_analyzed(&analyzed).unwrap()),
        protocol::Value::Vector(vec![protocol::Value::Int(2), protocol::Value::Int(10)])
    );
    assert_eq!(
        wire(&c.eval_string("[x (m) a/obsolete]").unwrap()),
        protocol::Value::Vector(vec![
            protocol::Value::Int(2),
            protocol::Value::Int(20),
            protocol::Value::Int(9)
        ])
    );
    c.collect_garbage();
    assert_eq!(
        wire(&c.eval_analyzed(&analyzed).unwrap()),
        protocol::Value::Vector(vec![protocol::Value::Int(2), protocol::Value::Int(10)])
    );
}

#[test]
fn nested_loads_restore_source_namespace_and_original_error_provenance() {
    let mut c = setup();
    let source = c.source(
        "outer.cljc",
        "(ns consumer) (require 'load.file) [*file* (ns-name *ns*) load.file/file]",
    );
    assert_eq!(
        wire(&c.eval_source(&source).unwrap()),
        protocol::Value::Vector(vec![
            protocol::Value::String("outer.cljc".encode_utf16().collect()),
            protocol::Value::Symbol(None, "consumer".encode_utf16().collect()),
            protocol::Value::String("load/file.cljc".encode_utf16().collect()),
        ])
    );
    c.set_source_provider(|_: &LoadRequest| {
        Ok(Some(SourceInput::new(
            "broken.clj",
            "(ns broken)\n(def x 1)\nmissing",
        )))
    });
    let source = c.source("caller.clj", "(require 'broken)");
    let error = c.eval_source(&source).unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert_eq!(error.span.source().name(), "broken.clj");
    assert!(
        error
            .frames
            .iter()
            .any(|f| f.span.source().name() == "caller.clj")
    );
    assert_eq!(
        eval(&mut c, "(ns-name *ns*)"),
        ValueView::Symbol(Name::new(None, "consumer"))
    );
    let file = c
        .resolve_var(&Name::new(Some("clojure.core"), "*file*"))
        .unwrap()
        .unwrap();
    assert_eq!(
        c.var_root(file.var_id().unwrap()).unwrap().inspect(),
        ValueView::Nil
    );
    c.collect_garbage();
    assert!(error.to_string().contains("broken.clj"));
}

#[test]
fn libspec_validation_precedes_provider_effects() {
    for source in [
        "(require)",
        "(require :reload)",
        "(require :verbose 'good)",
        "(require '[good :as])",
        "(require '[good :as 1])",
        "(require '[good :as a :as b])",
        "(require '[good :as a :as-alias b])",
        "(require '[good :refer 1])",
        "(require '[good :rename {x 1}])",
        "(require '[good :unknown []])",
        "(require '[prefix [foo.bar]])",
        "(require 'good '[bad :as])",
        "(require [1])",
        "(require '[good :reload :yes])",
        "(ns foo (:use bar))",
        "(ns foo (:require [bar :bad true]))",
    ] {
        let mut c = Context::new();
        c.set_source_provider(|_: &LoadRequest| {
            panic!("provider called for malformed declaration")
        });
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    let mut c = setup();
    c.eval_string("(require '[load.alpha :as a])").unwrap();
    assert!(c.eval_string("(require '[load.beta :as a])").is_err());
    assert!(
        c.eval_string("(require '[load.alpha :refer [absent]])")
            .is_err()
    );
}

#[test]
fn filesystem_adapter_is_explicit_supports_clj_cljc_and_reports_ambiguity() {
    let directory = std::env::temp_dir().join(format!(
        "light-cljs-loader-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let root = directory.join("one");
    let second = directory.join("two");
    std::fs::create_dir_all(root.join("example")).unwrap();
    std::fs::create_dir_all(second.join("example")).unwrap();
    std::fs::write(
        root.join("example/my_lib.cljc"),
        "(ns example.my-lib) (def x #?(:clj 42 :cljs 0))",
    )
    .unwrap();
    let provider = FileSourceProvider::new([root.clone(), second.clone()]);
    let mut c = Context::new();
    c.set_source_provider(provider.clone());
    assert_eq!(
        eval(&mut c, "(require 'example.my-lib) example.my-lib/x"),
        ValueView::Integer(42)
    );
    std::fs::write(second.join("example/my_lib.clj"), "(ns example.my-lib)").unwrap();
    let request = LoadRequest {
        namespace: Name::new(None, "example.my-lib"),
        requesting_namespace: Name::new(None, "user"),
        requesting_file: None,
        reload: Reload::None,
    };
    assert!(
        matches!(provider.load(&request), Err(LoadError::AmbiguousSources(paths)) if paths.len() == 2)
    );
    std::fs::remove_file(second.join("example/my_lib.clj")).unwrap();
    std::fs::write(root.join("example/my_lib.clj"), "(ns example.my-lib)").unwrap();
    assert!(matches!(
        provider.load(&request),
        Err(LoadError::AmbiguousSources(_))
    ));
    for invalid in ["../escape", "foo..bar", "foo\\bar", "foo:bar"] {
        let request = LoadRequest {
            namespace: Name::new(None, invalid),
            ..request.clone()
        };
        assert!(matches!(
            provider.load(&request),
            Err(LoadError::InvalidLibspec(_))
        ));
    }
    let request = LoadRequest {
        namespace: Name::new(None, "missing"),
        ..request
    };
    assert!(provider.load(&request).unwrap().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

static RESET: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "reset-loader",
        arity: Arity::Exact(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, _| {
        c.reset();
        Ok(c.nil())
    },
};
#[test]
fn reset_in_a_loaded_callback_returns_wrong_owner_and_clears_provider() {
    let mut c = Context::new();
    c.register_native(&RESET);
    c.set_source_provider(|_: &LoadRequest| {
        Ok(Some(SourceInput::new(
            "reset.clj",
            "(ns reset) (user/reset-loader)",
        )))
    });
    let error = c.eval_string("(require 'reset)").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    c.collect_garbage();
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "reset")),
        LoadState::Unloaded
    );
    assert_eq!(eval(&mut c, "(+ 1 2)"), ValueView::Integer(3));
    assert!(matches!(
        c.eval_string("(require 'reset)").unwrap_err().kind,
        E::Load(LoadError::MissingNamespace(_))
    ));
}

#[test]
fn resource_and_host_failures_unwind_loading_state_and_permit_later_retry() {
    for limits in [
        ResourceLimits {
            fuel: Some(50),
            ..ResourceLimits::default()
        },
        ResourceLimits {
            allocation: Some(250),
            ..ResourceLimits::default()
        },
        ResourceLimits {
            call_depth: 4,
            ..ResourceLimits::default()
        },
    ] {
        let mut c = Context::new();
        c.set_source_provider(|r: &LoadRequest| {
            let name = String::from_utf16_lossy(r.namespace.name());
            Ok(Some(SourceInput::new(
                format!("{name}.clj"),
                format!("(ns {name}) (require '{}x)", name),
            )))
        });
        c.set_resource_limits(limits);
        let error = c
            .eval_string("(try (require 'chain) (catch Exception e :caught))")
            .unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{error}");
        assert_ne!(
            c.namespace_load_state(&Name::new(None, "chain")),
            LoadState::Loading
        );
        c.set_resource_limits(ResourceLimits::default());
        c.set_source_provider(|_: &LoadRequest| {
            Ok(Some(SourceInput::new("chain.clj", "(ns chain) (def x 5)")))
        });
        assert_eq!(
            eval(&mut c, "(require 'chain) chain/x"),
            ValueView::Integer(5)
        );
    }
    let mut c = Context::new();
    c.set_source_provider(|_: &LoadRequest| Err(LoadError::Provider("test failure".into())));
    assert_eq!(
        c.eval_string("(try (require 'bad) (catch Exception e nil))")
            .unwrap_err()
            .class(),
        ErrorClass::Host
    );
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "bad")),
        LoadState::Failed
    );
}

#[test]
fn source_restoration_never_swallows_allocation_exhaustion() {
    let mut baseline = Context::new();
    baseline.eval_string("*file*").unwrap();
    baseline.eval_string("1").unwrap();
    let required = baseline.resource_usage().allocation;
    for allocation in 0..required + 3 {
        let mut c = Context::new();
        c.eval_string("*file*").unwrap();
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..ResourceLimits::default()
        });
        let result = c.eval_string("1");
        assert_eq!(
            result.is_ok(),
            allocation >= required,
            "allocation {allocation}, required {required}"
        );
        match result {
            Ok(_) => assert!(c.resource_usage().allocation <= allocation),
            Err(error) => assert_eq!(error.class(), ErrorClass::Control),
        }
    }
}

#[test]
fn cancellation_from_provider_prevents_source_initialization_and_can_be_retried() {
    use light_cljs::interpreter::CancellationToken;
    let mut c = Context::new();
    let token = CancellationToken::default();
    c.set_cancellation_token(Some(token.clone()));
    c.set_source_provider(move |_: &LoadRequest| {
        token.cancel();
        Ok(Some(SourceInput::new(
            "cancel.clj",
            "(ns cancel) (def x 1)",
        )))
    });
    assert_eq!(
        c.eval_string("(require 'cancel)").unwrap_err().class(),
        ErrorClass::Control
    );
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "cancel")),
        LoadState::Failed
    );
    assert!(c.find_namespace(&Name::new(None, "cancel")).is_none());
    c.set_cancellation_token(None);
    c.set_source_provider(|_: &LoadRequest| {
        Ok(Some(SourceInput::new(
            "cancel.clj",
            "(ns cancel) (def x 2)",
        )))
    });
    assert_eq!(
        eval(&mut c, "(require 'cancel) cancel/x"),
        ValueView::Integer(2)
    );
}

#[test]
fn alias_repair_obeys_small_allocation_limits() {
    for allocation in [0, 1, 20, 50, 100, 200, 400, 800, 1600, 3200] {
        let mut c = Context::new();
        c.set_source_provider(|_: &LoadRequest| {
            Ok(Some(SourceInput::new(
                "target.clj",
                "(ns target) (def x 1)",
            )))
        });
        c.eval_string("(require '[target :as t])").unwrap();
        for i in 0..16 {
            c.eval_string(&format!("(ns alias{i}) (require '[target :as t])"))
                .unwrap();
        }
        c.set_source_provider(|_: &LoadRequest| {
            Ok(Some(SourceInput::new("target.clj", "(ns target) (/ 1 0)")))
        });
        assert!(c.eval_string("(require 'target :reload)").is_err());
        c.set_source_provider(|_: &LoadRequest| {
            Ok(Some(SourceInput::new(
                "target.clj",
                "(ns target) (def x 2)",
            )))
        });
        c.set_resource_limits(ResourceLimits {
            allocation: Some(allocation),
            ..ResourceLimits::default()
        });
        let result = c.eval_string("(require 'target)");
        if let Err(e) = result {
            assert_eq!(e.class(), ErrorClass::Control, "{allocation}: {e}");
        }
        c.set_resource_limits(ResourceLimits::default());
        c.eval_string("(require 'target)").unwrap();
        c.collect_garbage();
        assert_eq!(eval(&mut c, "t/x"), ValueView::Integer(2));
    }
}
