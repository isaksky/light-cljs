//! The simulated compiler host (pellet clj-quickjs-9): immutable worlds,
//! separate result kinds per definition, producers scheduled through the
//! dependency cells, and deterministic failures.
//!
//! Callables are JavaScript source compiled by this runtime (synthetic
//! fixtures in `tests/fixtures/js/`, not light-cljs output). Calls between
//! them use the host's stand-in tail-call convention (`host::sim` module
//! docs) until the VM has call instructions.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clj_quickjs::frontend::compile_function;
use clj_quickjs::host::sim::{
    ANALYSIS, BINDING, CALLABLE, INITIALIZED, Producer, SimError, SimEvent, SimHost, WorldSpec,
};
use clj_quickjs::{
    CellStatus, DependencyId, DependencyKey, Primitive, ResultKind, Rooted, Runtime, ValueView,
    WorldId,
};

const IS_EVEN: DependencyId = DependencyId::new(1);
const IS_ODD: DependencyId = DependencyId::new(2);
const PARITY: DependencyId = DependencyId::new(3);
const CONFIG: DependencyId = DependencyId::new(4);
const USER: DependencyId = DependencyId::new(5);
const UNDEFINED_NAME: DependencyId = DependencyId::new(99);

fn view(v: &Rooted) -> ValueView {
    v.view()
}

/// A Rust producer that counts its runs and returns the vector `[tag]`.
fn tagged(rt_runs: &Arc<AtomicUsize>, tag: f64) -> Producer {
    let runs = Arc::clone(rt_runs);
    Producer::rust(move |rt, _| {
        runs.fetch_add(1, Ordering::SeqCst);
        Ok(rt.scope(|s| {
            let n = s.primitive(Primitive::Number(tag));
            let v = s.new_vector(&[n]).unwrap();
            s.persist(v)
        }))
    })
}

fn first_item(rt: &Runtime, v: &Rooted) -> ValueView {
    rt.scope(|s| {
        let v = s.local(v).unwrap();
        s.view(s.nth(v, 0).unwrap())
    })
}

fn scheduled(host: &SimHost, key: DependencyKey) -> usize {
    host.events()
        .iter()
        .filter(|e| matches!(e, SimEvent::Scheduled { key: k, .. } if *k == key))
        .count()
}

fn activations(host: &SimHost) -> usize {
    host.events()
        .iter()
        .filter(|e| matches!(e, SimEvent::Activation { .. }))
        .count()
}

/// isEven/isOdd refer to each other; `parity` is initialized by calling
/// `isEven(10)`; isOdd's initializer depends on `parity`'s. Building either
/// callable needs only the other's binding, so the initializers are never
/// needed to call them.
fn parity_world(init_runs: &Arc<AtomicUsize>) -> WorldSpec {
    let even = compile_function(include_str!("fixtures/js/sim_is_even.js")).unwrap();
    let odd = compile_function(include_str!("fixtures/js/sim_is_odd.js")).unwrap();
    let mut spec = WorldSpec::new();
    spec.define(IS_EVEN, "isEven")
        .produce(
            IS_EVEN,
            CALLABLE,
            Producer::callable(even, Some(IS_ODD)).needing(IS_ODD, BINDING),
        )
        .produce(IS_EVEN, INITIALIZED, tagged(init_runs, 0.0))
        .produce(
            IS_EVEN,
            ANALYSIS,
            // Rust work: the number of definitions isEven refers to.
            Producer::rust(|rt, refs| Ok(rt.root(Primitive::Number(refs.len() as f64))))
                .needing(IS_ODD, BINDING),
        );
    spec.define(IS_ODD, "isOdd")
        .produce(
            IS_ODD,
            CALLABLE,
            Producer::callable(odd, Some(IS_EVEN)).needing(IS_EVEN, BINDING),
        )
        .produce(
            IS_ODD,
            INITIALIZED,
            tagged(init_runs, 1.0).needing(PARITY, INITIALIZED),
        );
    spec.define(PARITY, "parity").produce(
        PARITY,
        INITIALIZED,
        Producer::invoke(IS_EVEN, Primitive::Number(10.0)),
    );
    spec
}

#[test]
fn mutually_recursive_callables_run_without_waiting_for_initializers() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let init_runs = Arc::new(AtomicUsize::new(0));
    let w = host.add_world(parity_world(&init_runs));
    let key = |def, kind| DependencyKey::new(w, def, kind);

    let parity = host.need(w, PARITY, INITIALIZED);
    let value = parity.result.expect("parity initializes");
    assert_eq!(view(&value), ValueView::Bool(true), "isEven(10)");
    assert_eq!(parity.log.edges(), [key(PARITY, INITIALIZED)]);
    assert_eq!(parity.log.waiting(), None);

    // Eleven activations alternated between the two VM callables.
    assert_eq!(activations(&host), 11);
    // Neither function's initializer was needed, produced or created.
    assert_eq!(init_runs.load(Ordering::SeqCst), 0);
    for def in [IS_EVEN, IS_ODD] {
        assert!(matches!(
            rt.cell_status(key(def, INITIALIZED)),
            CellStatus::Absent
        ));
    }
    // The recursive reference graph is recorded (each callable used the
    // other's binding; the initializer used both callables) but nothing is
    // waiting: recursion is not a wait cycle.
    let used = |k: DependencyKey| host.producer_log(k).unwrap().edges().to_vec();
    assert_eq!(used(key(IS_EVEN, CALLABLE)), [key(IS_ODD, BINDING)]);
    assert_eq!(used(key(IS_ODD, CALLABLE)), [key(IS_EVEN, BINDING)]);
    assert_eq!(
        used(key(PARITY, INITIALIZED)),
        [key(IS_EVEN, CALLABLE), key(IS_ODD, CALLABLE)]
    );
    assert_eq!(host.parked(), 0);
    // One producer per key, however often a callable was needed.
    for def in [IS_EVEN, IS_ODD] {
        assert_eq!(scheduled(&host, key(def, CALLABLE)), 1);
        assert_eq!(scheduled(&host, key(def, BINDING)), 1);
    }

    // The published callables are directly invocable.
    for (def, n, expected) in [
        (IS_EVEN, 7.0, false),
        (IS_ODD, 7.0, true),
        (IS_ODD, 0.0, false),
    ] {
        let answer = host.invoke(w, def, &rt.root(Primitive::Number(n)));
        assert_eq!(view(&answer.result.unwrap()), ValueView::Bool(expected));
    }
    assert_eq!(scheduled(&host, key(IS_EVEN, CALLABLE)), 1);

    // isOdd's initializer depends on parity, which is already initialized.
    let odd_init = host.need(w, IS_ODD, INITIALIZED).result.unwrap();
    assert_eq!(first_item(&rt, &odd_init), ValueView::Number(1.0));
    assert_eq!(init_runs.load(Ordering::SeqCst), 1);

    // Analysis is its own result, produced by Rust work.
    let analysis = host.need(w, IS_EVEN, ANALYSIS).result.unwrap();
    assert_eq!(view(&analysis), ValueView::Number(1.0));
    assert!(matches!(
        rt.cell_status(key(IS_EVEN, INITIALIZED)),
        CellStatus::Absent
    ));
}

#[test]
fn every_callable_reader_gets_the_one_published_callable() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let w = host.add_world(parity_world(&Arc::new(AtomicUsize::new(0))));
    let a = host.need(w, IS_EVEN, CALLABLE).result.unwrap();
    let b = host.need(w, IS_EVEN, CALLABLE).result.unwrap();
    assert!(a.same_object(&b));
    let odd = host.need(w, IS_ODD, CALLABLE).result.unwrap();
    assert!(!a.same_object(&odd), "each callable has its own identity");
}

#[test]
fn worlds_and_result_kinds_name_distinct_cells_and_old_results_stay_stable() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let runs = Arc::new(AtomicUsize::new(0));
    let mut v1 = WorldSpec::new();
    v1.define(CONFIG, "config")
        .produce(CONFIG, INITIALIZED, tagged(&runs, 1.0))
        .produce(
            CONFIG,
            ANALYSIS,
            Producer::rust(|rt, _| Ok(rt.root(Primitive::Number(10.0)))),
        );
    let mut v2 = v1.clone();
    v2.produce(CONFIG, INITIALIZED, tagged(&runs, 2.0));
    let w1 = host.add_world(v1.clone());
    let w2 = host.add_world(v2);
    assert_ne!(w1, w2);
    let key = |w: WorldId, kind: ResultKind| DependencyKey::new(w, CONFIG, kind);

    // World 2 completes first; world 1's cell is untouched by it.
    let in_w2 = host.need(w2, CONFIG, INITIALIZED).result.unwrap();
    assert_eq!(first_item(&rt, &in_w2), ValueView::Number(2.0));
    assert!(matches!(
        rt.cell_status(key(w1, INITIALIZED)),
        CellStatus::Absent
    ));
    let in_w1 = host.need(w1, CONFIG, INITIALIZED).result.unwrap();
    assert_eq!(first_item(&rt, &in_w1), ValueView::Number(1.0));
    assert!(!in_w1.same_object(&in_w2));

    // Other kinds of the same definition are other cells.
    let analysis = host.need(w1, CONFIG, ANALYSIS).result.unwrap();
    assert_eq!(view(&analysis), ValueView::Number(10.0));
    let binding = host.need(w1, CONFIG, BINDING).result.unwrap();
    assert_eq!(view(&binding), ValueView::Bool(true));
    let missing = host.need(w1, CONFIG, CALLABLE).result.unwrap_err();
    assert_eq!(
        missing,
        SimError::MissingResult {
            key: key(w1, CALLABLE)
        }
    );
    assert!(
        matches!(rt.cell_status(key(w1, INITIALIZED)), CellStatus::Ready(v) if v.same_object(&in_w1))
    );
    assert!(
        matches!(rt.cell_status(key(w1, ANALYSIS)), CellStatus::Ready(v) if v.view() == ValueView::Number(10.0))
    );
    assert!(matches!(
        rt.cell_status(key(w2, ANALYSIS)),
        CellStatus::Absent
    ));

    // A newer world without the definition does not reset world 1.
    let mut v3 = v1;
    v3.remove(CONFIG);
    let w3 = host.add_world(v3);
    assert_eq!(
        host.need(w3, CONFIG, INITIALIZED).result.unwrap_err(),
        SimError::MissingDefinition {
            key: key(w3, INITIALIZED)
        }
    );
    drop((in_w2, analysis, binding));
    rt.collect().unwrap();
    let again = host.need(w1, CONFIG, INITIALIZED).result.unwrap();
    assert!(again.same_object(&in_w1), "completed results are stable");
    assert_eq!(runs.load(Ordering::SeqCst), 2, "one run per world");
    assert_eq!(scheduled(&host, key(w1, INITIALIZED)), 1);
    assert_eq!(host.world(w1).unwrap().name(CONFIG), Some("config"));
    assert_eq!(host.world(w3).unwrap().name(CONFIG), None);
}

#[test]
fn missing_definitions_and_results_fail_deterministically() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    spec.define(USER, "user").produce(
        USER,
        INITIALIZED,
        Producer::rust(|rt, _| Ok(rt.root(Primitive::Null))).needing(UNDEFINED_NAME, BINDING),
    );
    let w = host.add_world(spec);
    let key = |def, kind| DependencyKey::new(w, def, kind);

    // An unknown name settles as an explicit error after one drive, never
    // as an indefinitely pending result; asking again reads the same cell.
    let missing = key(UNDEFINED_NAME, BINDING);
    for _ in 0..2 {
        let answer = host.need(w, UNDEFINED_NAME, BINDING);
        assert_eq!(
            answer.result.unwrap_err(),
            SimError::MissingDefinition { key: missing }
        );
        assert_eq!(answer.log.waiting(), None);
    }
    assert_eq!(scheduled(&host, missing), 1);
    assert!(
        matches!(rt.cell_status(missing), CellStatus::Failed(e) if e.view() == ValueView::Number(2.0))
    );

    // A producer whose input is missing fails, naming the input.
    let user = host.need(w, USER, INITIALIZED);
    assert_eq!(
        user.result.unwrap_err(),
        SimError::InputFailed {
            key: key(USER, INITIALIZED),
            input: missing
        }
    );
    assert_eq!(
        host.failure(key(USER, INITIALIZED)),
        Some(&SimError::InputFailed {
            key: key(USER, INITIALIZED),
            input: missing
        })
    );

    // A known definition without that result, and an unknown callee.
    assert_eq!(
        host.need(w, USER, ANALYSIS).result.unwrap_err(),
        SimError::MissingResult {
            key: key(USER, ANALYSIS)
        }
    );
    assert_eq!(
        host.invoke(w, UNDEFINED_NAME, &rt.root(Primitive::Number(1.0)))
            .result
            .unwrap_err(),
        SimError::MissingDefinition {
            key: key(UNDEFINED_NAME, CALLABLE)
        }
    );
    // A world the host never registered.
    let stranger = rt.new_world();
    assert_eq!(
        host.need(stranger, USER, BINDING).result.unwrap_err(),
        SimError::UnknownWorld {
            key: DependencyKey::new(stranger, USER, BINDING)
        }
    );
    assert_eq!(host.parked(), 0, "every request settled");
}

#[test]
fn a_failing_producer_settles_its_cell_as_failed_and_wakes_its_waiters() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "config")
        .produce(
            CONFIG,
            INITIALIZED,
            Producer::rust(|_, _| Err("config file unreadable".to_owned())),
        )
        .produce(
            CONFIG,
            ANALYSIS,
            // Task-owned mutation cannot be published as a result.
            Producer::rust(|rt, _| Ok(rt.scope(|s| s.persist(s.new_object())))),
        );
    spec.define(USER, "user").produce(
        USER,
        INITIALIZED,
        Producer::rust(|rt, _| Ok(rt.root(Primitive::Null))).needing(CONFIG, INITIALIZED),
    );
    let w = host.add_world(spec);
    let config = DependencyKey::new(w, CONFIG, INITIALIZED);
    let user = DependencyKey::new(w, USER, INITIALIZED);

    let answer = host.need(w, USER, INITIALIZED);
    assert_eq!(
        answer.result.unwrap_err(),
        SimError::InputFailed {
            key: user,
            input: config
        }
    );
    assert_eq!(
        host.failure(config),
        Some(&SimError::ProducerFailed {
            key: config,
            message: "config file unreadable".to_owned()
        })
    );
    assert!(
        matches!(rt.cell_status(config), CellStatus::Failed(e) if e.view() == ValueView::Number(4.0))
    );
    assert!(matches!(rt.cell_status(user), CellStatus::Failed(_)));

    // The user producer parked on config and was woken by its failure.
    let user_job = host
        .events()
        .iter()
        .find_map(|e| match e {
            SimEvent::Parked { task, waiting } if *waiting == config => Some(*task),
            _ => None,
        })
        .expect("the user producer waited on config");
    let order: Vec<_> = host
        .events()
        .iter()
        .filter(|e| match e {
            SimEvent::Settled { key, .. } => *key == config,
            SimEvent::Woken { task } => *task == user_job,
            _ => false,
        })
        .cloned()
        .collect();
    assert_eq!(
        order,
        [
            SimEvent::Settled {
                key: config,
                ok: false
            },
            SimEvent::Woken { task: user_job }
        ]
    );

    let analysis = DependencyKey::new(w, CONFIG, ANALYSIS);
    let err = host.need(w, CONFIG, ANALYSIS).result.unwrap_err();
    assert!(
        matches!(&err, SimError::ProducerFailed { key, message } if *key == analysis && message.contains("not shareable")),
        "{err:?}"
    );
    assert!(matches!(rt.cell_status(analysis), CellStatus::Failed(_)));
}

#[test]
fn a_genuine_wait_cycle_is_reported_as_stalled_instead_of_hanging() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    let pass_through = || Producer::rust(|_, inputs| Ok(inputs[0].clone()));
    spec.define(CONFIG, "a").produce(
        CONFIG,
        INITIALIZED,
        pass_through().needing(USER, INITIALIZED),
    );
    spec.define(USER, "b").produce(
        USER,
        INITIALIZED,
        pass_through().needing(CONFIG, INITIALIZED),
    );
    let w = host.add_world(spec);
    let answer = host.need(w, CONFIG, INITIALIZED);
    let a = DependencyKey::new(w, CONFIG, INITIALIZED);
    assert_eq!(answer.result.unwrap_err(), SimError::Stalled { waiting: a });
    assert_eq!(answer.log.waiting(), Some(a));
    // The request and both producers are parked on each other's cells.
    assert_eq!(host.parked(), 3);
}

#[test]
fn a_runaway_tail_call_chain_hits_the_activation_limit() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    host.set_activation_limit(5);
    let w = host.add_world(parity_world(&Arc::new(AtomicUsize::new(0))));
    // -1 never reaches the base case.
    let answer = host.invoke(w, IS_EVEN, &rt.root(Primitive::Number(-1.0)));
    assert!(matches!(
        answer.result.unwrap_err(),
        SimError::ActivationLimit { limit: 5, .. }
    ));
    assert_eq!(activations(&host), 5);
}

#[test]
fn calling_a_non_callable_or_a_failing_function_fails_the_call() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "config").produce(
        CONFIG,
        CALLABLE,
        // A number is not a callable this host published.
        Producer::rust(|rt, _| Ok(rt.root(Primitive::Number(0.0)))),
    );
    let broken = compile_function("function broken(n) { return n + missing; }").unwrap();
    spec.define(USER, "broken")
        .produce(USER, CALLABLE, Producer::callable(broken, None));
    let w = host.add_world(spec);
    let arg = rt.root(Primitive::Number(1.0));
    assert_eq!(
        host.invoke(w, CONFIG, &arg).result.unwrap_err(),
        SimError::NotCallable {
            callable: DependencyKey::new(w, CONFIG, CALLABLE)
        }
    );
    let err = host.invoke(w, USER, &arg).result.unwrap_err();
    assert!(
        matches!(&err, SimError::ActivationFailed { callable, .. } if *callable == DependencyKey::new(w, USER, CALLABLE)),
        "{err:?}"
    );
}
