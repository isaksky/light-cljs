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
    CellStatus, Completion, DependencyId, DependencyKey, Primitive, ResultKind, Rooted, Runtime,
    TaskId, ValueView, WorldId,
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
    // Jobs really waited (on each callable and binding as it was first
    // produced), yet the recursion closed no wait cycle and left no wait.
    assert!(host.events().iter().any(
        |e| matches!(e, SimEvent::Parked { waiting, .. } if *waiting == key(IS_ODD, CALLABLE))
    ));
    assert!(host.cycles().is_empty());
    assert_eq!(host.graph().active_waits(), 0);
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

fn pass_through() -> Producer {
    Producer::rust(|_, inputs| Ok(inputs[0].clone()))
}

/// The tasks that parked on `key`, in order.
fn parked_on(host: &SimHost, key: DependencyKey) -> Vec<TaskId> {
    host.events()
        .iter()
        .filter_map(|e| match e {
            SimEvent::Parked { task, waiting } if *waiting == key => Some(*task),
            _ => None,
        })
        .collect()
}

/// a's initializer needs b's and b's needs a's: an execution cycle (not a
/// reference cycle). The newest demand (b's producer needing a) is refused
/// with the cycle; b fails with it, which fails a's input, which answers the
/// request. Everything settles and nothing stays parked or waiting, while
/// every recorded use survives.
#[test]
fn a_genuine_wait_cycle_is_reported_and_settles_every_waiter() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
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
    let a = DependencyKey::new(w, CONFIG, INITIALIZED);
    let b = DependencyKey::new(w, USER, INITIALIZED);
    let answer = host.need(w, CONFIG, INITIALIZED);
    assert_eq!(
        answer.result.unwrap_err(),
        SimError::InputFailed { key: a, input: b }
    );
    assert_eq!(answer.log.waiting(), None);

    // One cycle, reported once, with a readable path.
    let cycles = host.cycles();
    assert_eq!(cycles.len(), 1);
    let cycle = cycles[0].clone();
    let steps = cycle.steps();
    assert_eq!(cycle.keys(), [a, b]);
    let (b_job, a_job) = (steps[0].task, steps[1].task);
    assert_eq!(
        (steps[0].producer_task, steps[1].producer_task),
        (a_job, b_job)
    );
    assert_eq!(steps[0].site.description, "b.initialized: input 0");
    assert_eq!(steps[1].site.description, "a.initialized: input 0");
    assert_eq!(
        host.failure(b),
        Some(&SimError::WaitCycle {
            key: b,
            cycle: cycle.clone()
        })
    );
    assert_eq!(
        host.failure(b).unwrap().to_string(),
        format!(
            "{b}: wait cycle of 2 demand(s): {b_job} (b.initialized: input 0) waits on {a}, \
             produced by {} on {a_job} -> {a_job} (a.initialized: input 0) waits on {b}, \
             produced by {} on {b_job}",
            steps[0].producer, steps[1].producer
        )
    );

    // Everything settled: no parked job, no active wait.
    assert!(matches!(rt.cell_status(a), CellStatus::Failed(_)));
    assert!(matches!(rt.cell_status(b), CellStatus::Failed(_)));
    assert_eq!(host.parked(), 0);
    assert_eq!(host.graph().active_waits(), 0);
    // a's producer really waited on b; b's producer never parked.
    assert_eq!(parked_on(&host, b), [a_job]);
    assert_eq!(parked_on(&host, a), [answer.task]);

    // Failure removed the waits but kept the history.
    let graph = host.graph();
    assert_eq!(graph.uses(a_job), [b]);
    assert_eq!(graph.uses(b_job), [a]);
    assert_eq!(graph.uses(answer.task), [a]);
    assert_eq!((graph.waiting(a_job), graph.waiting(b_job)), (None, None));
    for (key, used) in [(a, b), (b, a)] {
        let log = host.producer_log(key).unwrap();
        assert_eq!(log.edges(), [used]);
        assert_eq!(log.waiting(), None, "the refused wait was withdrawn");
    }
}

#[test]
fn a_definition_waiting_on_itself_is_a_cycle_of_one() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "a").produce(
        CONFIG,
        INITIALIZED,
        pass_through().needing(CONFIG, INITIALIZED),
    );
    let w = host.add_world(spec);
    let a = DependencyKey::new(w, CONFIG, INITIALIZED);
    let err = host.need(w, CONFIG, INITIALIZED).result.unwrap_err();
    let SimError::WaitCycle { key, cycle } = &err else {
        panic!("{err:?}");
    };
    assert_eq!(*key, a);
    assert_eq!(cycle.keys(), [a]);
    assert_eq!(cycle.steps()[0].task, cycle.steps()[0].producer_task);
    assert_eq!(host.parked(), 0);
}

/// A cycle through different result kinds and a call: a's initializer calls
/// f, f's callable needs g's analysis, and g's analysis needs a's
/// initializer. The path names each task, demand, world and result kind.
#[test]
fn a_cycle_through_a_call_and_several_result_kinds_names_each_demand() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    let f = compile_function("function f(n) { return n; }").unwrap();
    spec.define(CONFIG, "a").produce(
        CONFIG,
        INITIALIZED,
        Producer::invoke(USER, Primitive::Number(1.0)),
    );
    spec.define(USER, "f").produce(
        USER,
        CALLABLE,
        Producer::callable(f, None).needing(PARITY, ANALYSIS),
    );
    spec.define(PARITY, "g").produce(
        PARITY,
        ANALYSIS,
        pass_through().needing(CONFIG, INITIALIZED),
    );
    let w = host.add_world(spec);
    let a = DependencyKey::new(w, CONFIG, INITIALIZED);
    let f_callable = DependencyKey::new(w, USER, CALLABLE);
    let g = DependencyKey::new(w, PARITY, ANALYSIS);

    let answer = host.need(w, CONFIG, INITIALIZED);
    assert_eq!(
        answer.result.unwrap_err(),
        SimError::InputFailed {
            key: a,
            input: f_callable
        }
    );
    let cycles = host.cycles();
    assert_eq!(cycles.len(), 1);
    let steps = cycles[0].steps();
    let path: Vec<_> = steps
        .iter()
        .map(|s| (s.site.description.as_str(), s.key.world(), s.key.kind()))
        .collect();
    assert_eq!(
        path,
        [
            ("g.analysis: input 0", w, INITIALIZED),
            ("a.initialized: call of f.callable", w, CALLABLE),
            ("f.callable: input 0", w, ANALYSIS),
        ]
    );
    assert_eq!(cycles[0].keys(), [a, f_callable, g]);
    for i in 0..3 {
        assert_eq!(steps[i].producer_task, steps[(i + 1) % 3].task);
    }
    assert_eq!(
        host.failure(f_callable),
        Some(&SimError::InputFailed {
            key: f_callable,
            input: g
        })
    );
    assert!(matches!(host.failure(g), Some(SimError::WaitCycle { key, .. }) if *key == g));
    assert_eq!(activations(&host), 0, "f never became callable");
    assert_eq!((host.parked(), host.graph().active_waits()), (0, 0));
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

#[test]
fn a_result_of_another_runtime_settles_the_cell_as_failed() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "config").produce(
        CONFIG,
        INITIALIZED,
        Producer::rust(|_, _| Ok(Runtime::new().root(Primitive::Null))),
    );
    let w = host.add_world(spec);
    let key = DependencyKey::new(w, CONFIG, INITIALIZED);
    for _ in 0..2 {
        let err = host.need(w, CONFIG, INITIALIZED).result.unwrap_err();
        assert!(
            matches!(&err, SimError::ProducerFailed { key: k, message } if *k == key && message.contains("different runtime")),
            "{err:?}"
        );
    }
    assert!(matches!(rt.cell_status(key), CellStatus::Failed(_)));
    assert_eq!(scheduled(&host, key), 1);
    assert_eq!(host.parked(), 0);
}

fn identity_world(host: &mut SimHost) -> WorldId {
    let mut spec = WorldSpec::new();
    let identity = compile_function("function id(x) { return x; }").unwrap();
    spec.define(USER, "id")
        .produce(USER, CALLABLE, Producer::callable(identity, None));
    spec.define(CONFIG, "alias");
    host.add_world(spec)
}

#[test]
fn invoke_checks_publication_of_the_argument_and_keeps_its_identity() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let w = identity_world(&mut host);
    let (shareable, unshareable) = rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        let inner = s.new_vector(&[one]).unwrap();
        let ok = s.new_vector(&[inner]).unwrap();
        let plain = s.new_object();
        let nested = s.new_vector(&[plain]).unwrap();
        let bad = s.new_vector(&[nested]).unwrap();
        (s.persist(ok), s.persist(bad))
    });
    let back = host.invoke(w, USER, &shareable).result.unwrap();
    assert!(back.same_object(&shareable), "no copy: the same graph");
    let err = host.invoke(w, USER, &unshareable).result.unwrap_err();
    assert!(
        matches!(&err, SimError::ArgumentNotShareable { callable, .. } if *callable == DependencyKey::new(w, USER, CALLABLE)),
        "{err:?}"
    );
    assert_eq!(activations(&host), 1, "the rejected argument ran nothing");
}

#[test]
fn a_stalled_invocation_resumed_later_drops_its_answer() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let w = identity_world(&mut host);
    // Someone outside the host claims alias's callable, so the call stalls.
    let alias = DependencyKey::new(w, CONFIG, CALLABLE);
    let claim = rt.claim(alias).unwrap();
    let graph = rt.scope(|s| {
        let n = s.primitive(Primitive::Number(5.0));
        let v = s.new_vector(&[n]).unwrap();
        s.persist(v)
    });
    let weak = graph.downgrade();
    let stalled = host.invoke(w, CONFIG, &graph);
    assert_eq!(
        stalled.result.unwrap_err(),
        SimError::Stalled { waiting: alias }
    );
    drop(graph);

    let token = host.need(w, USER, CALLABLE).result.unwrap();
    let woken = rt
        .complete(alias, claim.producer(), Completion::Ready(&token))
        .unwrap()
        .task_ids();
    assert_eq!(woken, [stalled.task]);
    host.wake(&woken);
    host.run_until_idle();
    assert_eq!(activations(&host), 1, "the resumed call ran");
    assert_eq!(host.parked(), 0);
    rt.collect().unwrap();
    assert!(!weak.is_live(), "the late answer was not retained");
}

#[test]
fn cancelled_inputs_fail_dependents_without_running_their_work() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let runs = Arc::new(AtomicUsize::new(0));
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "config");
    spec.define(USER, "user")
        .produce(
            USER,
            INITIALIZED,
            tagged(&runs, 1.0).needing(CONFIG, INITIALIZED),
        )
        .produce(USER, ANALYSIS, Producer::invoke(CONFIG, Primitive::Null));
    let w = host.add_world(spec);
    let config = |kind| DependencyKey::new(w, CONFIG, kind);
    for kind in [INITIALIZED, CALLABLE] {
        let claim = rt.claim(config(kind)).unwrap();
        let woken = rt
            .complete(config(kind), claim.producer(), Completion::Cancelled)
            .unwrap();
        assert!(woken.deliveries().is_empty(), "no job waits yet");
    }

    assert_eq!(
        host.need(w, CONFIG, INITIALIZED).result.unwrap_err(),
        SimError::Cancelled {
            key: config(INITIALIZED)
        }
    );
    let user = DependencyKey::new(w, USER, INITIALIZED);
    assert_eq!(
        host.need(w, USER, INITIALIZED).result.unwrap_err(),
        SimError::InputCancelled {
            key: user,
            input: config(INITIALIZED)
        }
    );
    assert_eq!(runs.load(Ordering::SeqCst), 0, "the body never ran");
    assert!(matches!(rt.cell_status(user), CellStatus::Failed(_)));

    // An initializer calling a cancelled callable fails the same way.
    let analysis = DependencyKey::new(w, USER, ANALYSIS);
    assert_eq!(
        host.need(w, USER, ANALYSIS).result.unwrap_err(),
        SimError::InputCancelled {
            key: analysis,
            input: config(CALLABLE)
        }
    );
    assert!(matches!(rt.cell_status(analysis), CellStatus::Failed(_)));
    assert_eq!(activations(&host), 0);
}

#[test]
fn a_producer_keeps_its_ordered_inputs_across_several_waits() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let runs = Arc::new(AtomicUsize::new(0));
    let unused = Arc::new(AtomicUsize::new(0));
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "a")
        .produce(CONFIG, INITIALIZED, tagged(&unused, 1.0));
    spec.define(IS_EVEN, "b")
        .produce(IS_EVEN, INITIALIZED, tagged(&unused, 2.0));
    let counted = Arc::clone(&runs);
    spec.define(USER, "pair").produce(
        USER,
        INITIALIZED,
        Producer::rust(move |rt, inputs| {
            counted.fetch_add(1, Ordering::SeqCst);
            let tags: Vec<_> = inputs.iter().map(|v| first_item(rt, v)).collect();
            assert_eq!(tags, [ValueView::Number(1.0), ValueView::Number(2.0)]);
            Ok(rt.root(Primitive::Number(inputs.len() as f64)))
        })
        .needing(CONFIG, INITIALIZED)
        .needing(IS_EVEN, INITIALIZED),
    );
    let w = host.add_world(spec);
    let user = DependencyKey::new(w, USER, INITIALIZED);
    let a = DependencyKey::new(w, CONFIG, INITIALIZED);
    let b = DependencyKey::new(w, IS_EVEN, INITIALIZED);

    let pair = host.need(w, USER, INITIALIZED).result.unwrap();
    assert_eq!(pair.view(), ValueView::Number(2.0));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the work ran once");
    let log = host.producer_log(user).unwrap();
    assert_eq!((log.edges(), log.waiting()), (&[a, b][..], None));
    // The producer parked once on each pending input, in declaration order.
    let waits: Vec<_> = host
        .events()
        .iter()
        .filter_map(|e| match e {
            SimEvent::Parked { waiting, .. } if *waiting == a || *waiting == b => Some(*waiting),
            _ => None,
        })
        .collect();
    assert_eq!(waits, [a, b]);
}

/// A ready read is a recorded use with no wait: nothing parks, and the
/// graph records the use.
#[test]
fn ready_reads_are_recorded_without_an_active_wait() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let w = identity_world(&mut host);
    let callable = DependencyKey::new(w, USER, CALLABLE);
    let first = host.need(w, USER, CALLABLE);
    assert!(first.result.is_ok());
    assert_eq!(parked_on(&host, callable), [first.task], "the first waited");

    let second = host.need(w, USER, CALLABLE);
    assert!(second.result.is_ok());
    assert_eq!(
        parked_on(&host, callable),
        [first.task],
        "the second did not"
    );
    assert_eq!(second.log.edges(), [callable]);
    let graph = host.graph();
    assert_eq!(graph.uses(second.task), [callable]);
    assert_eq!(graph.waiting(second.task), None);
    assert_eq!(graph.uses(first.task), [callable]);
    assert_eq!(graph.active_waits(), 0);
    assert!(host.cycles().is_empty());
}

/// An embedder cancels a cell that a parked request waits on (its producer
/// was claimed outside the host, so no cycle can be found): the wake removes
/// the stale wait edge, and the use stays recorded.
#[test]
fn cancellation_removes_the_stale_wait_but_keeps_the_use() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let w = identity_world(&mut host);
    let alias = DependencyKey::new(w, CONFIG, CALLABLE);
    let _claim = rt.claim(alias).unwrap();
    let stalled = host.need(w, CONFIG, CALLABLE);
    assert_eq!(
        stalled.result.unwrap_err(),
        SimError::Stalled { waiting: alias }
    );
    assert_eq!(host.graph().waiting(stalled.task), Some(alias));
    assert_eq!(host.graph().waiters(alias), [stalled.task]);
    assert_eq!(
        host.graph()
            .waiting_site(stalled.task)
            .map(|s| s.description.as_str()),
        Some("request for alias.callable")
    );

    let woken = rt.cancel_cell(alias).unwrap().task_ids();
    assert_eq!(woken, [stalled.task]);
    host.wake(&woken);
    let graph = host.graph();
    assert_eq!(graph.waiting(stalled.task), None);
    assert_eq!(graph.waiters(alias), []);
    assert!(graph.used(stalled.task, alias));
    host.run_until_idle();
    assert_eq!(host.parked(), 0);
    assert_eq!(host.graph().uses(stalled.task), [alias]);
    assert!(host.cycles().is_empty());
}

/// a waits on b, b on c, and c needs d, then a. d's Rust work cancels b
/// from outside the host's bookkeeping and keeps the wake IDs instead of
/// handing them to `SimHost::wake`, so the graph still holds a's wait on b
/// when c demands a. The candidate cycle c → a → b → c runs through a
/// cancelled cell: it is not genuine and must not be reported. Once the
/// wakes arrive, the cancellation propagates as ordinary input errors.
#[test]
fn external_settlement_breaks_a_candidate_cycle() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let b_key: Arc<std::sync::Mutex<Option<DependencyKey>>> = Arc::default();
    let held_wakes: Arc<std::sync::Mutex<Vec<TaskId>>> = Arc::default();
    let mut spec = WorldSpec::new();
    spec.define(CONFIG, "a").produce(
        CONFIG,
        INITIALIZED,
        pass_through().needing(USER, INITIALIZED),
    );
    spec.define(USER, "b").produce(
        USER,
        INITIALIZED,
        pass_through().needing(PARITY, INITIALIZED),
    );
    spec.define(PARITY, "c").produce(
        PARITY,
        INITIALIZED,
        pass_through()
            .needing(IS_EVEN, INITIALIZED)
            .needing(CONFIG, INITIALIZED),
    );
    let (cancel, held) = (Arc::clone(&b_key), Arc::clone(&held_wakes));
    spec.define(IS_EVEN, "d").produce(
        IS_EVEN,
        INITIALIZED,
        Producer::rust(move |rt, _| {
            let b = cancel.lock().unwrap().expect("b's key is set");
            let woken = rt.cancel_cell(b).map_err(|e| e.to_string())?;
            held.lock().unwrap().extend(woken.task_ids());
            Ok(rt.root(Primitive::Number(0.0)))
        }),
    );
    let w = host.add_world(spec);
    let key = |def| DependencyKey::new(w, def, INITIALIZED);
    let (a, b, c) = (key(CONFIG), key(USER), key(PARITY));
    *b_key.lock().unwrap() = Some(b);

    let first = host.need(w, CONFIG, INITIALIZED);
    // c parked on a (a's producer still parked on b): no cycle reported.
    assert_eq!(first.result.unwrap_err(), SimError::Stalled { waiting: a });
    assert!(host.cycles().is_empty(), "{:?}", host.cycles());
    assert!(matches!(rt.cell_status(b), CellStatus::Cancelled));
    let held = std::mem::take(&mut *held_wakes.lock().unwrap());
    assert_eq!(held.len(), 1, "a's producer was b's only waiter");

    host.wake(&held);
    host.run_until_idle();
    assert!(host.cycles().is_empty());
    assert_eq!(
        host.failure(a),
        Some(&SimError::InputCancelled { key: a, input: b })
    );
    assert_eq!(
        host.failure(c),
        Some(&SimError::InputFailed { key: c, input: a })
    );
    assert_eq!((host.parked(), host.graph().active_waits()), (0, 0));
}

/// The demand that closes the cycle is a call: a's callable needs b's
/// initializer, which invokes a. Requesting a's callable makes b's call
/// demand the newest, so b's own cell fails with the cycle.
#[test]
fn a_call_demand_that_closes_a_cycle_fails_its_produced_cell() {
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    let mut spec = WorldSpec::new();
    let f = compile_function("function a(n) { return n; }").unwrap();
    spec.define(CONFIG, "a").produce(
        CONFIG,
        CALLABLE,
        Producer::callable(f, None).needing(USER, INITIALIZED),
    );
    spec.define(USER, "b").produce(
        USER,
        INITIALIZED,
        Producer::invoke(CONFIG, Primitive::Number(1.0)),
    );
    let w = host.add_world(spec);
    let a = DependencyKey::new(w, CONFIG, CALLABLE);
    let b = DependencyKey::new(w, USER, INITIALIZED);

    let answer = host.need(w, CONFIG, CALLABLE);
    assert_eq!(
        answer.result.unwrap_err(),
        SimError::InputFailed { key: a, input: b }
    );
    let cycles = host.cycles();
    assert_eq!(cycles.len(), 1);
    let cycle = cycles[0].clone();
    assert_eq!(cycle.keys(), [a, b]);
    let steps = cycle.steps();
    assert_eq!(
        steps[0].site.description,
        "b.initialized: call of a.callable"
    );
    assert_eq!(steps[1].site.description, "a.callable: input 0");
    assert_eq!(
        host.failure(b),
        Some(&SimError::WaitCycle {
            key: b,
            cycle: cycle.clone()
        })
    );
    assert!(matches!(rt.cell_status(a), CellStatus::Failed(_)));
    assert!(matches!(rt.cell_status(b), CellStatus::Failed(_)));
    let (b_job, a_job) = (steps[0].task, steps[1].task);
    assert_eq!(host.graph().uses(b_job), [a]);
    assert_eq!(host.graph().uses(a_job), [b]);
    assert_eq!(host.producer_log(b).unwrap().waiting(), None);
    assert_eq!(activations(&host), 0);
    assert_eq!((host.parked(), host.graph().active_waits()), (0, 0));
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_default_locale_resolution() {
    use clj_quickjs::{Budget, RunOutcome};
    let rt = Runtime::new();
    let mut host = SimHost::new(&rt);
    host.set_default_locale("de");
    for source in [
        "new Intl.PluralRules().resolvedOptions().locale === 'de'",
        "new Intl.PluralRules('xx').resolvedOptions().locale === 'de'",
    ] {
        let code = clj_quickjs::frontend::compile_script("plural-default", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(10000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{source}: {other:?}"),
        }
        assert!(task.dependency_log().edges().is_empty());
    }
}

/// Synthetic fixture, not light-cljs output. Locale resolution follows both
/// the matcher getter and its string coercion, even across dependency waits.
#[test]
fn intl_default_locale_resolution_after_matcher_wait() {
    use clj_quickjs::{Budget, RunOutcome};
    for locales in ["undefined", "'xx'"] {
        let rt = Runtime::new();
        let mut host = SimHost::new(&rt);
        host.set_default_locale("en-US");
        let source = format!(
            "let p=new Intl.PluralRules({locales},{{get localeMatcher(){{__rt.need(2121,1);return {{toString(){{__rt.need(2122,1);return 'lookup';}}}};}}}}); p.resolvedOptions().locale === 'de'"
        );
        let code =
            clj_quickjs::frontend::compile_script("plural-default-after-matcher", &source).unwrap();
        let mut task = rt.spawn(&code, &[]).unwrap();
        for locale in ["fr", "de"] {
            assert!(matches!(
                task.run_with(Budget::instructions(10000), &mut host)
                    .unwrap(),
                RunOutcome::Waiting(_)
            ));
            host.set_default_locale(locale);
            rt.collect().unwrap();
            let request = task.take_producer_requests().pop().unwrap();
            let zero = rt.root(Primitive::Number(0.0));
            task = rt
                .complete(request.key(), request.producer(), Completion::Ready(&zero))
                .unwrap()
                .into_runnable()
                .pop()
                .unwrap();
        }
        match task
            .run_with(Budget::instructions(10000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{other:?}"),
        }
    }
    struct NoLocale;
    impl clj_quickjs::Host for NoLocale {
        fn effect(&mut self, _: clj_quickjs::HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
        fn default_locale(&mut self) -> String {
            panic!("an abrupt matcher must not consult the host")
        }
    }
    for matcher in ["throw 9;", "return 'bogus';"] {
        let rt = Runtime::new();
        let source = format!(
            "let caught=false; try {{new Intl.PluralRules(undefined,{{get localeMatcher(){{{matcher}}}}});}} catch(e){{caught=e === 9 || e instanceof RangeError;}} caught"
        );
        let code = clj_quickjs::frontend::compile_script("plural-abrupt-matcher", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        assert!(
            matches!(task.run_with(Budget::instructions(10000), &mut NoLocale).unwrap(),RunOutcome::Complete(value) if value.view()==ValueView::Bool(true))
        );
    }
}

// Date field rows adapted from QuickJS tests/test_intl.js at 7d1c13c5d8f8.
// QuickJS Javascript Engine
//
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.
/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_date_time_format_date_fields() {
    use clj_quickjs::{Budget, RunOutcome, host::TimeZone};
    for zone in [TimeZone::UTC, TimeZone::fixed_offset_seconds(3600).unwrap()] {
        let rt = Runtime::new();
        let mut host = clj_quickjs::host::sim::SimTimeHost::new(1506098258091.0, zone);
        let source = r#"
        let d = new Date(2017, 11, 12, 6, 0, 0, 0);
        if (new Intl.DateTimeFormat('en', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "December 12, 2017") throw "en dateTimeFormat long month";
        if (new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric' }).format(d) !== "Dec 12") throw "en dateTimeFormat short month";
        if (new Intl.DateTimeFormat('en', { weekday: 'long' }).format(d) !== "Tuesday") throw "en dateTimeFormat weekday";
        if (new Intl.DateTimeFormat('en', { year: 'numeric', era: 'short' }).format(d) !== "2017 AD") throw "en dateTimeFormat era";
        if (new Intl.DateTimeFormat('en-GB', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "December 12, 2017") throw "en-GB dateTimeFormat";
        if (new Intl.DateTimeFormat('de', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "Dezember 12, 2017") throw "de dateTimeFormat";
        if (new Intl.DateTimeFormat('de', { month: 'short', day: 'numeric' }).format(d) !== "Dez. 12") throw "de dateTimeFormat short month";
        if (new Intl.DateTimeFormat('de', { weekday: 'long' }).format(d) !== "Dienstag") throw "de weekday";
        if (new Intl.DateTimeFormat('de', { weekday: 'narrow' }).format(d) !== "D") throw "de narrow weekday";
        if (new Intl.DateTimeFormat('de', { month: 'narrow' }).format(d) !== "D") throw "de narrow month";
        if (new Intl.DateTimeFormat('de', { year: 'numeric', era: 'short' }).format(d) !== "2017 n. Chr.") throw "de era";
        if (new Intl.DateTimeFormat('fr', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "décembre 12, 2017") throw "fr dateTimeFormat";
        if (new Intl.DateTimeFormat('fr', { month: 'short', day: 'numeric' }).format(d) !== "déc. 12") throw "fr short month";
        if (new Intl.DateTimeFormat('fr', { weekday: 'long' }).format(d) !== "mardi") throw "fr weekday";
        if (new Intl.DateTimeFormat('fr', { weekday: 'narrow' }).format(d) !== "M") throw "fr narrow weekday";
        if (new Intl.DateTimeFormat('fr', { month: 'narrow' }).format(d) !== "D") throw "fr narrow month";
        if (new Intl.DateTimeFormat('fr', { year: 'numeric', era: 'short' }).format(d) !== "2017 ap. J.-C.") throw "fr era";
        if (new Intl.DateTimeFormat('es', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "diciembre 12, 2017") throw "es dateTimeFormat";
        if (new Intl.DateTimeFormat('ja', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "12月 12, 2017") throw "ja dateTimeFormat";
        if (new Intl.DateTimeFormat('ja', { weekday: 'long' }).format(d) !== "火曜日") throw "ja weekday";
        if (new Intl.DateTimeFormat('ja', { weekday: 'narrow' }).format(d) !== "火") throw "ja narrow weekday";
        if (new Intl.DateTimeFormat('ja', { month: 'narrow' }).format(d) !== "12") throw "ja narrow month";
        if (new Intl.DateTimeFormat('ja', { year: 'numeric', era: 'short' }).format(d) !== "2017 西暦") throw "ja era";
        if (new Intl.DateTimeFormat('zh', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "十二月 12, 2017") throw "zh dateTimeFormat";
        if (new Intl.DateTimeFormat('zh', { year: 'numeric', era: 'short' }).format(d) !== "2017 公元") throw "zh era";
        if (new Intl.DateTimeFormat('zh', { weekday: 'narrow' }).format(d) !== "二") throw "zh narrow weekday";
        if (new Intl.DateTimeFormat('zh', { month: 'narrow' }).format(d) !== "12") throw "zh narrow month";
        if (new Intl.DateTimeFormat('zh-Hant', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "12月 12, 2017") throw "zh-Hant dateTimeFormat";
        if (new Intl.DateTimeFormat('zh-Hant', { year: 'numeric', era: 'short' }).format(d) !== "2017 西元") throw "zh-Hant era";
        if (new Intl.DateTimeFormat('ar', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "ديسمبر ١٢, ٢٠١٧") throw "ar dateTimeFormat";
        if (new Intl.DateTimeFormat('ar', { weekday: 'long' }).format(d) !== "الثلاثاء") throw "ar weekday";
        if (new Intl.DateTimeFormat('ar', { year: 'numeric', era: 'short' }).format(d) !== "٢٠١٧ م") throw "ar era";
        if (new Intl.DateTimeFormat('ru', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "декабря 12, 2017") throw "ru dateTimeFormat";
        if (new Intl.DateTimeFormat('ru', { weekday: 'long' }).format(d) !== "вторник") throw "ru weekday";
        if (new Intl.DateTimeFormat('ru', { year: 'numeric', era: 'short' }).format(d) !== "2017 н. э.") throw "ru era";
        if (new Intl.DateTimeFormat('pt-BR', { month: 'long', day: 'numeric', year: 'numeric' }).format(d) !== "dezembro 12, 2017") throw "pt-BR dateTimeFormat";
        if (new Intl.DateTimeFormat('en', {timeZone:'UTC', year:'numeric'}).resolvedOptions().timeZone !== 'UTC') throw 'zone';
        let caught=false;
        try {new Intl.DateTimeFormat().format(NaN);} catch(e) {caught=e instanceof RangeError;}
        if (!caught) throw 'NaN';
        if (new Intl.DateTimeFormat('en', {timeZone:'UTC',year:'numeric',month:'short',day:'numeric'}).format() !== 'Sep 22, 2017') throw 'clock';
        true
        "#;
        let code = clj_quickjs::frontend::compile_script("dtf-date-fields", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{zone:?}: {other:?}"),
        }
        assert!(task.dependency_log().edges().is_empty());
    }
}

/// Synthetic fixtures, not light-cljs output. Named zones are available to the
/// formatter only when selected by the embedding host, with the Date rules.
#[test]
fn intl_date_time_format_host_zones_and_transitions() {
    use clj_quickjs::{
        Budget, RunOutcome,
        host::{TimeZone, sim::SimTimeHost},
    };
    let rt = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::named("America/New_York").unwrap());
    let source = r#"
        let f=new Intl.DateTimeFormat('en',{year:'numeric',month:'2-digit',day:'2-digit'});
        if (f.resolvedOptions().timeZone !== 'America/New_York') throw 'zone';
        if (f.format(Date.UTC(2017,2,12,6,59,59)) !== '03/12/2017') throw 'spring before';
        if (f.format(Date.UTC(2017,2,12,7)) !== '03/12/2017') throw 'spring after';
        if (f.format(Date.UTC(2017,2,13,3,59,59)) !== '03/12/2017') throw 'spring midnight before';
        if (f.format(Date.UTC(2017,2,13,4)) !== '03/13/2017') throw 'spring midnight after';
        if (f.format(Date.UTC(2017,10,5,5,59,59)) !== '11/05/2017') throw 'fall before';
        if (f.format(Date.UTC(2017,10,5,6)) !== '11/05/2017') throw 'fall after';
        if (f.format(Date.UTC(2017,10,6,4,59,59)) !== '11/05/2017') throw 'fall midnight before';
        if (f.format(Date.UTC(2017,10,6,5)) !== '11/06/2017') throw 'fall midnight after';
        let time=new Intl.DateTimeFormat('en',{hour:'numeric',minute:'numeric',second:'numeric',hourCycle:'h23'});
        if (time.format(Date.UTC(2017,2,12,6,59,59)) !== '01:59:59') throw 'spring time before';
        if (time.format(Date.UTC(2017,2,12,7)) !== '03:00:00') throw 'spring time after';
        if (time.format(Date.UTC(2017,10,5,5,59,59)) !== '01:59:59') throw 'fall time before';
        if (time.format(Date.UTC(2017,10,5,6)) !== '01:00:00') throw 'fall time after';
        let named=new Intl.DateTimeFormat('en',{timeZone:'US/Eastern',year:'numeric'});
        let lower=new Intl.DateTimeFormat('en',{timeZone:'america/new_york',year:'numeric'});
        let alias=new Intl.DateTimeFormat('en',{timeZone:'us/eastern',year:'numeric'});
        named.resolvedOptions().timeZone === 'America/New_York' && named.format(0) === '1969'
        && lower.resolvedOptions().timeZone === 'America/New_York' && lower.format(0) === '1969'
        && alias.resolvedOptions().timeZone === 'America/New_York' && alias.format(0) === '1969'
    "#;
    let code = clj_quickjs::frontend::compile_script("dtf-host-zone", source).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    match task
        .run_with(Budget::instructions(100000), &mut host)
        .unwrap()
    {
        RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
        other => panic!("{other:?}"),
    }
    // Date and all subsequent formatters keep the runtime's first zone.
    host.zone = TimeZone::UTC;
    let code = clj_quickjs::frontend::compile_script("dtf-same-zone", "new Intl.DateTimeFormat().resolvedOptions().timeZone === 'America/New_York' && new Date(1970,0,1).getTime() === 18000000").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    assert!(
        matches!(task.run_with(Budget::instructions(10000), &mut host).unwrap(), RunOutcome::Complete(value) if value.view() == ValueView::Bool(true))
    );
    assert_eq!(host.clock_reads(), 0);
}

/// Synthetic fixture, not light-cljs output. Host zones may retain second
/// precision, while explicit Intl offset options must use minute precision.
#[test]
fn intl_date_time_format_fixed_second_host_zone() {
    use clj_quickjs::{
        Budget, RunOutcome,
        host::{TimeZone, sim::SimTimeHost},
    };
    let rt = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::fixed_offset_seconds(3630).unwrap());
    let code = clj_quickjs::frontend::compile_script("dtf-fixed-second-zone", r#"
        let d=new Intl.DateTimeFormat('en',{year:'numeric'});
        let minute=new Intl.DateTimeFormat('en',{year:'numeric',timeZone:'+01:00'});
        let caught=false;
        try {new Intl.DateTimeFormat('en',{timeZone:'+01:00:30'});} catch(e) {caught=e instanceof RangeError;}
        caught && d.resolvedOptions().timeZone === '+01:00:30'
        && d.format(Date.UTC(2017,11,31,22,59,30)) === '2018'
        && minute.format(Date.UTC(2017,11,31,22,59,30)) === '2017'
    "#).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    assert!(
        matches!(task.run_with(Budget::instructions(10000), &mut host).unwrap(), RunOutcome::Complete(value) if value.view() == ValueView::Bool(true))
    );
}

/// Synthetic fixtures, not light-cljs output. ResolveLocale observes the host
/// after the saved matcher getter and coercion have completed.
#[test]
fn intl_date_time_format_host_default_locale() {
    use clj_quickjs::{Budget, RunOutcome};
    for locales in ["undefined", "'xx'"] {
        let rt = Runtime::new();
        let mut host = SimHost::new(&rt);
        host.set_default_locale("de");
        let source = format!(
            "let d=new Intl.DateTimeFormat({locales},{{month:'long'}}); d.resolvedOptions().locale === 'de' && d.format(0) === 'Januar'"
        );
        let code = clj_quickjs::frontend::compile_script("dtf-host-locale", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        assert!(
            matches!(task.run_with(Budget::instructions(10000), &mut host).unwrap(), RunOutcome::Complete(value) if value.view() == ValueView::Bool(true))
        );
        assert!(task.dependency_log().edges().is_empty());

        let source = format!(
            "let d=new Intl.DateTimeFormat({locales},{{get localeMatcher(){{__rt.need(2151,1);return {{toString(){{__rt.need(2152,1);return 'lookup';}}}};}},month:'long'}}); d.resolvedOptions().locale === 'de' && d.format(0) === 'Januar'"
        );
        let code =
            clj_quickjs::frontend::compile_script("dtf-locale-after-matcher", &source).unwrap();
        host.set_default_locale("en-US");
        let mut task = rt.spawn(&code, &[]).unwrap();
        for locale in ["fr", "de"] {
            assert!(matches!(
                task.run_with(Budget::instructions(10000), &mut host)
                    .unwrap(),
                RunOutcome::Waiting(_)
            ));
            host.set_default_locale(locale);
            rt.collect().unwrap();
            let request = task.take_producer_requests().pop().unwrap();
            let zero = rt.root(Primitive::Number(0.0));
            task = rt
                .complete(request.key(), request.producer(), Completion::Ready(&zero))
                .unwrap()
                .into_runnable()
                .pop()
                .unwrap();
        }
        assert!(
            matches!(task.run_with(Budget::instructions(10000), &mut host).unwrap(), RunOutcome::Complete(value) if value.view() == ValueView::Bool(true))
        );
    }
}

// Time rows adapted from QuickJS tests/test_intl.js at 7d1c13c5d8f8;
// covered by the QuickJS copyright and MIT notice above the date-field test.
/// Synthetic fixture, not light-cljs output. The snapshot-output policy selects
/// Spanish U+202F inside a. m. and Japanese's trailing 午前, as QuickJS asserts.
#[test]
fn intl_date_time_format_time_fields() {
    use clj_quickjs::{Budget, RunOutcome, host::TimeZone};
    for zone in [TimeZone::UTC, TimeZone::fixed_offset_seconds(3600).unwrap()] {
        let rt = Runtime::new();
        let mut host = clj_quickjs::host::sim::SimTimeHost::new(1506098258091.0, zone);
        let source = r#"
            let d=new Date(2017,11,12,6,0,0,0);
            if(new Intl.DateTimeFormat('en',{hour:'numeric',minute:'numeric',hour12:true}).format(d) !== '6:00 AM') throw 'en time';
            if(new Intl.DateTimeFormat('en-GB',{hour:'numeric',minute:'numeric',hour12:true}).format(d) !== '6:00 am') throw 'en-GB time';
            if(new Intl.DateTimeFormat('es',{hour:'numeric',minute:'numeric',hour12:true}).format(d) !== '6:00 a. m.') throw 'es QuickJS snapshot';
            if(new Intl.DateTimeFormat('ja',{hour:'numeric',minute:'numeric',hour12:true}).format(d) !== '6:00 午前') throw 'ja QuickJS snapshot';
            if(new Intl.DateTimeFormat('ar',{hour:'numeric',minute:'numeric',hour12:true}).format(d) !== '٦:٠٠ ص') throw 'ar time';
            if(new Intl.DateTimeFormat('en',{hour:'numeric',minute:'2-digit',hourCycle:'h23',timeZone:'UTC'}).format(Date.UTC(2017,0,1,18,5)) !== '18:05') throw 'h23';
            if(new Intl.DateTimeFormat('en',{hour:'numeric',hour12:true,timeZone:'UTC'}).format(Date.UTC(2017,0,1,0,30)) !== '12 AM') throw 'midnight';
            if(new Intl.DateTimeFormat('en',{hour:'numeric',hour12:true}).resolvedOptions().hourCycle !== 'h12') throw 'resolved cycle';
            let caught=false;
            try {new Intl.DateTimeFormat('en',{hourCycle:'h25'});} catch(e){caught=e instanceof RangeError;}
            if(!caught) throw 'invalid cycle';
            true
        "#;
        let code = clj_quickjs::frontend::compile_script("dtf-time-fields", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{zone:?}: {other:?}"),
        }
        assert!(task.dependency_log().edges().is_empty());
    }
}

// Date style rows adapted from QuickJS tests/test_intl.js at 7d1c13c5d8f8;
// covered by the QuickJS copyright and MIT notice above the date-field test.
/// Synthetic fixture, not light-cljs output.
#[test]
fn intl_date_time_format_date_style() {
    use clj_quickjs::{Budget, RunOutcome, host::TimeZone};
    for zone in [TimeZone::UTC, TimeZone::fixed_offset_seconds(3600).unwrap()] {
        let rt = Runtime::new();
        let mut host = clj_quickjs::host::sim::SimTimeHost::new(0.0, zone);
        let source = r#"
            let d = new Date(2023, 0, 15);
            if (new Intl.DateTimeFormat('en-US', {dateStyle:'short'}).format(d) !== '1/15/23') throw 'en-US short';
            if (new Intl.DateTimeFormat('en-GB', {dateStyle:'short'}).format(d) !== '15/01/2023') throw 'en-GB short';
            if (new Intl.DateTimeFormat('de', {dateStyle:'short'}).format(d) !== '15.01.23') throw 'de short';
            if (new Intl.DateTimeFormat('fr', {dateStyle:'short'}).format(d) !== '15/01/2023') throw 'fr short';
            if (new Intl.DateTimeFormat('ja', {dateStyle:'short'}).format(d) !== '2023/01/15') throw 'ja short';
            if (new Intl.DateTimeFormat('zh', {dateStyle:'short'}).format(d) !== '2023/1/15') throw 'zh short';
            if (new Intl.DateTimeFormat('es', {dateStyle:'short'}).format(d) !== '15/1/23') throw 'es short';
            if (new Intl.DateTimeFormat('de', {dateStyle:'medium'}).format(d) !== '15.01.2023') throw 'de medium';
            if (new Intl.DateTimeFormat('de', {dateStyle:'long'}).format(d) !== '15. Januar 2023') throw 'de long';
            if (new Intl.DateTimeFormat('de', {dateStyle:'full'}).format(d) !== 'Sonntag, 15. Januar 2023') throw 'de full';
            let caught = false;
            try { new Intl.DateTimeFormat('en', {dateStyle:'short', year:'numeric'}); }
            catch(e) { caught = e instanceof TypeError; }
            if (!caught) throw 'mixed fields';
            caught = false;
            try { new Intl.DateTimeFormat('en', {dateStyle:'tiny'}); }
            catch(e) { caught = e instanceof RangeError; }
            if (!caught) throw 'invalid style';
            let r = new Intl.DateTimeFormat('de', {dateStyle:'long'}).resolvedOptions();
            if (r.dateStyle !== 'long' || r.year !== undefined || r.month !== undefined || r.day !== undefined || r.hourCycle !== undefined) throw 'resolved style';
            true
        "#;
        let code = clj_quickjs::frontend::compile_script("dtf-date-style", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{zone:?}: {other:?}"),
        }
        assert!(task.dependency_log().edges().is_empty());
    }
}

/// Synthetic fixtures, not light-cljs output; snapshot style patterns preserve
/// their literal spaces, punctuation and position of the day period.
#[test]
fn intl_date_time_format_time_style() {
    use clj_quickjs::{Budget, RunOutcome, host::TimeZone};
    for zone in [TimeZone::UTC, TimeZone::fixed_offset_seconds(3600).unwrap()] {
        let rt = Runtime::new();
        let mut host = clj_quickjs::host::sim::SimTimeHost::new(0.0, zone);
        let source = r#"
            let d = new Date(2023, 0, 15, 18, 5, 9);
            if (new Intl.DateTimeFormat('en', {timeStyle:'short'}).format(d) !== '6:05 PM') throw 'en short';
            if (new Intl.DateTimeFormat('de', {timeStyle:'medium'}).format(d) !== '18:05:09') throw 'de medium';
            if (new Intl.DateTimeFormat('ja', {timeStyle:'short'}).format(d) !== '18:05') throw 'ja short';
            if (new Intl.DateTimeFormat('zh', {timeStyle:'short'}).format(d) !== '18:05') throw 'zh short';
            if (new Intl.DateTimeFormat('en', {dateStyle:'short', timeStyle:'short'}).format(d) !== '1/15/23, 6:05 PM') throw 'combined';
            if (new Intl.DateTimeFormat('pt-BR', {dateStyle:'long'}).format(d) !== '15 de janeiro de 2023') throw 'quoted literals';
            if (new Intl.DateTimeFormat('ar', {timeStyle:'short'}).format(d) !== '٦:٠٥ م') throw 'digits';
            if (new Intl.DateTimeFormat('en', {timeStyle:'short', hourCycle:'h23'}).format(d) !== '18:05') throw 'suppressed period';
            if (new Intl.DateTimeFormat('de', {timeStyle:'short', hour12:true}).format(d) !== '06:05') throw 'snapshot hour width';
            let r = new Intl.DateTimeFormat('en', {timeStyle:'full'}).resolvedOptions();
            if (r.timeStyle !== 'full' || r.hourCycle !== 'h12' || !r.hour12 || r.hour !== undefined || r.minute !== undefined || r.second !== undefined || r.timeZoneName !== undefined) throw 'resolved time';
            if (new Intl.DateTimeFormat('en', {timeStyle:'long',timeZone:'UTC'}).format(0) !== '12:00:00 AM UTC') throw 'UTC long';
            if (new Intl.DateTimeFormat('en', {timeStyle:'full',timeZone:'UTC'}).format(0) !== '12:00:00 AM Coordinated Universal Time') throw 'UTC full';
            if (new Intl.DateTimeFormat('en', {timeStyle:'full',timeZone:'+01:30'}).format(0) !== '1:30:00 AM +01:30') throw 'offset full';
            true
        "#;
        let code = clj_quickjs::frontend::compile_script("dtf-time-style", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{zone:?}: {other:?}"),
        }
    }
}

/// Synthetic fixture, not light-cljs output. Full and long time styles use
/// the host zone's offset at the formatted instant, including DST boundaries.
#[test]
fn intl_date_time_format_style_zone_names() {
    use clj_quickjs::{Budget, RunOutcome, host::TimeZone};
    for (name, source) in [
        (
            "America/New_York",
            r#"
            let f = new Intl.DateTimeFormat('en-GB', {timeStyle:'long'});
            if (f.format(Date.UTC(2017,2,12,6,59,59)) !== '01:59:59 GMT-5') throw 'before DST';
            if (f.format(Date.UTC(2017,2,12,7)) !== '03:00:00 GMT-4') throw 'after DST';
            let full = new Intl.DateTimeFormat('en-GB', {timeStyle:'full'});
            if (full.format(Date.UTC(2017,2,12,7)) !== '03:00:00 GMT-04:00') throw 'full offset';
            true
        "#,
        ),
        (
            "Europe/Vienna",
            r#"
            let f = new Intl.DateTimeFormat('de', {timeStyle:'full'});
            if (f.format(Date.UTC(2023,0,15,12)) !== '13:00:00 Central European Standard Time') throw 'winter';
            if (f.format(Date.UTC(2023,6,15,12)) !== '14:00:00 Central European Summer Time') throw 'summer';
            true
        "#,
        ),
    ] {
        let rt = Runtime::new();
        let mut host =
            clj_quickjs::host::sim::SimTimeHost::new(0.0, TimeZone::named(name).unwrap());
        let code = clj_quickjs::frontend::compile_script("dtf-style-zone", source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        match task
            .run_with(Budget::instructions(100000), &mut host)
            .unwrap()
        {
            RunOutcome::Complete(value) => assert_eq!(value.view(), ValueView::Bool(true)),
            other => panic!("{name}: {other:?}"),
        }
    }
}
