//! Atoms through the embedding API and the VM atom intrinsics: identity
//! compare-and-set between two writers, single-writer publication, safe
//! load-to-root across replacement and collection, and publication checks
//! (`docs/design.md` §6; contract in `clj_quickjs::atom`).
//!
//! Programs are hand-assembled bytecode: early VM tests, not JavaScript
//! source support. Cross-thread orderings are fixed by channels with a
//! bounded wait; nothing relies on sleeps.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::{
    Assembler, AtomError, AtomErrorKind, Budget, CasOutcome, Executable, HandleError, Host,
    HostCall, Opcode, PathStep, Primitive, PublishError, Rooted, RunOutcome, Runtime, ValueView,
    VmErrorKind,
};

/// Bound for every cross-thread handshake: a broken handshake fails the test
/// instead of hanging it.
const HANDSHAKE: Duration = Duration::from_secs(10);

fn num(n: f64) -> Primitive {
    Primitive::Number(n)
}

fn vector(rt: &Runtime, items: &[f64]) -> Rooted {
    rt.scope(|s| {
        let items: Vec<_> = items.iter().map(|&n| s.primitive(num(n))).collect();
        let v = s.new_vector(&items).unwrap();
        s.persist(v)
    })
}

fn plain_object(rt: &Runtime) -> Rooted {
    rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    })
}

fn swapped(outcome: Result<CasOutcome, AtomError>) {
    match outcome {
        Ok(CasOutcome::Swapped) => {}
        other => panic!("expected Swapped, got {other:?}"),
    }
}

fn conflict(outcome: Result<CasOutcome, AtomError>) -> Rooted {
    match outcome {
        Ok(CasOutcome::Conflict { current }) => current,
        other => panic!("expected Conflict, got {other:?}"),
    }
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn failed_kind(outcome: RunOutcome) -> VmErrorKind {
    match outcome {
        RunOutcome::Failed(e) => e.kind(),
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// The items of a vector as views.
fn items(rt: &Runtime, v: &Rooted) -> Vec<ValueView> {
    rt.scope(|s| {
        let v = s.local(v).unwrap();
        let items = s.items(v).unwrap();
        items.into_iter().map(|i| s.view(i)).collect()
    })
}

/// `return [atom_load(arg0)...]`-style programs over one atom argument.
fn program(name: &str, params: u16, locals: u16, build: impl FnOnce(&mut Assembler)) -> Executable {
    let mut asm = Assembler::new();
    build(&mut asm);
    asm.finish(name, params, locals).unwrap()
}

#[test]
fn of_two_ordered_writers_from_the_same_snapshot_only_the_first_cas_succeeds() {
    let rt = Runtime::new();
    let initial = vector(&rt, &[0.0]);
    let atom = rt.new_atom(&initial).unwrap();
    drop(initial);
    let (a_done_tx, a_done_rx) = mpsc::channel::<()>();
    let (loaded_tx, loaded_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();

    let (rt, atom) = (&rt, &atom);
    let (a_new, b_current) = thread::scope(|t| {
        let writer_a = t.spawn(move || {
            let old = rt.atom_load(atom).unwrap();
            let new = vector(rt, &[1.0]);
            loaded_tx.send(()).unwrap();
            go_rx.recv_timeout(HANDSHAKE).expect("go");
            swapped(rt.atom_compare_and_set(atom, &old, &new));
            a_done_tx.send(()).unwrap();
            new
        });
        let writer_b = t.spawn(move || {
            // B snapshots the same old value before A writes...
            let old = rt.atom_load(atom).unwrap();
            let new = vector(rt, &[2.0]);
            loaded_rx.recv_timeout(HANDSHAKE).expect("A loaded");
            go_tx.send(()).unwrap();
            // ...and writes after A has written.
            a_done_rx.recv_timeout(HANDSHAKE).expect("A wrote");
            let current = conflict(rt.atom_compare_and_set(atom, &old, &new));
            assert!(!current.same_object(&old));
            current
        });
        (writer_a.join().unwrap(), writer_b.join().unwrap())
    });

    assert!(b_current.same_object(&a_new), "B is told what A installed");
    assert!(
        rt.atom_load(atom).unwrap().same_object(&a_new),
        "B changed nothing"
    );
    assert_eq!(items(rt, &b_current), [ValueView::Number(1.0)]);
}

#[test]
fn of_two_racing_writers_exactly_one_wins() {
    for _ in 0..50 {
        let rt = Runtime::new();
        let atom = rt.new_atom(&rt.root(num(0.0))).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let go_rx = std::sync::Mutex::new(go_rx);
        let outcomes: Vec<bool> = thread::scope(|t| {
            let writers: Vec<_> = [1.0, 2.0]
                .into_iter()
                .map(|n| {
                    let (rt, atom, ready_tx, go_rx) = (&rt, &atom, ready_tx.clone(), &go_rx);
                    t.spawn(move || {
                        let old = rt.atom_load(atom).unwrap();
                        let new = rt.root(num(n));
                        ready_tx.send(()).unwrap();
                        go_rx.lock().unwrap().recv_timeout(HANDSHAKE).expect("go");
                        matches!(
                            rt.atom_compare_and_set(atom, &old, &new),
                            Ok(CasOutcome::Swapped)
                        )
                    })
                })
                .collect();
            for _ in 0..2 {
                ready_rx.recv_timeout(HANDSHAKE).expect("ready");
            }
            go_tx.send(()).unwrap();
            go_tx.send(()).unwrap();
            writers.into_iter().map(|w| w.join().unwrap()).collect()
        });
        assert_eq!(
            outcomes.iter().filter(|&&won| won).count(),
            1,
            "{outcomes:?}"
        );
        let winner = if outcomes[0] { 1.0 } else { 2.0 };
        assert_eq!(
            rt.atom_load(&atom).unwrap().view(),
            ValueView::Number(winner)
        );
    }
}

#[test]
fn cas_compares_identity_and_same_value_never_structure() {
    let rt = Runtime::new();
    let held = vector(&rt, &[1.0, 2.0]);
    let twin = vector(&rt, &[1.0, 2.0]);
    rt.scope(|s| {
        let (a, b) = (s.local(&held).unwrap(), s.local(&twin).unwrap());
        assert!(s.value_equals(a, b), "structurally equal");
    });
    let atom = rt.new_atom(&held).unwrap();
    let new = vector(&rt, &[3.0]);
    let current = conflict(rt.atom_compare_and_set(&atom, &twin, &new));
    assert!(
        current.same_object(&held),
        "an equal but distinct object does not match"
    );
    swapped(rt.atom_compare_and_set(&atom, &held, &new));

    // Numbers compare by SameValue: NaN matches NaN, +0 and -0 differ.
    let nan = rt.new_atom(&rt.root(num(f64::NAN))).unwrap();
    swapped(rt.atom_compare_and_set(&nan, &rt.root(num(-f64::NAN)), &rt.root(num(1.0))));
    let zero = rt.new_atom(&rt.root(num(0.0))).unwrap();
    let current = conflict(rt.atom_compare_and_set(&zero, &rt.root(num(-0.0)), &rt.root(num(1.0))));
    assert_eq!(current.view(), ValueView::Number(0.0));
    swapped(rt.atom_compare_and_set(&zero, &rt.root(num(0.0)), &rt.root(Primitive::Null)));
    let current = conflict(rt.atom_compare_and_set(
        &zero,
        &rt.root(Primitive::Undefined),
        &rt.root(num(1.0)),
    ));
    assert_eq!(current.view(), ValueView::Null, "null is not undefined");

    // The same holds for the VM intrinsic: `atom_cas` against a twin fails.
    let atom = rt.new_atom(&held).unwrap();
    let cas = program("cas-twin", 3, 3, |asm| {
        asm.get_local(0)
            .get_local(1)
            .get_local(2)
            .op(Opcode::AtomCas);
        asm.new_vector(2).op(Opcode::Return);
    });
    let task = rt.spawn(&cas, &[&atom, &twin, &new]).unwrap();
    let result = complete(task.run().unwrap());
    rt.scope(|s| {
        let r = s.local(&result).unwrap();
        let witness = s.nth(r, 0).unwrap();
        assert!(s.same_object(witness, s.local(&held).unwrap()));
        assert_eq!(s.view(s.nth(r, 1).unwrap()), ValueView::Bool(false));
    });
    let task = rt.spawn(&cas, &[&atom, &held, &new]).unwrap();
    let result = complete(task.run().unwrap());
    assert_eq!(items(&rt, &result)[1], ValueView::Bool(true));
    assert!(rt.atom_load(&atom).unwrap().same_object(&new));
}

#[test]
fn a_loaded_old_value_survives_replacement_and_collection_until_released() {
    let rt = Runtime::new();
    // The old value is a small graph: a vector holding a vector.
    let (initial, inner_weak) = rt.scope(|s| {
        let leaf = s.primitive(num(7.0));
        let inner = s.new_vector(&[leaf]).unwrap();
        let outer = s.new_vector(&[inner, leaf]).unwrap();
        (s.persist(outer), s.downgrade(inner))
    });
    let atom = rt.new_atom(&initial).unwrap();
    let old_weak = initial.downgrade();
    drop(initial);
    let (loaded_tx, loaded_rx) = mpsc::channel::<()>();
    let (replaced_tx, replaced_rx) = mpsc::channel::<()>();
    let (released_tx, released_rx) = mpsc::channel::<()>();

    thread::scope(|t| {
        let (rt, atom, old_weak) = (&rt, &atom, &old_weak);
        t.spawn(move || {
            let held = rt.atom_load(atom).unwrap();
            loaded_tx.send(()).unwrap();
            replaced_rx.recv_timeout(HANDSHAKE).expect("replaced");
            // Still the same, fully readable graph.
            assert!(held.same_object(&old_weak.upgrade().unwrap()));
            rt.scope(|s| {
                let outer = s.local(&held).unwrap();
                let inner = s.nth(outer, 0).unwrap();
                assert_eq!(s.view(s.nth(inner, 0).unwrap()), ValueView::Number(7.0));
            });
            drop(held);
            released_tx.send(()).unwrap();
        });
        // Another worker replaces the atom and collects while the reader
        // holds the old value.
        loaded_rx.recv_timeout(HANDSHAKE).expect("loaded");
        let old = rt.atom_load(atom).unwrap();
        swapped(rt.atom_compare_and_set(atom, &old, &vector(rt, &[8.0])));
        drop(old);
        rt.collect().unwrap();
        assert!(
            old_weak.is_live() && inner_weak.is_live(),
            "the reader's root keeps it"
        );
        replaced_tx.send(()).unwrap();
        released_rx.recv_timeout(HANDSHAKE).expect("released");
    });

    rt.collect().unwrap();
    assert_eq!(old_weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    assert!(!inner_weak.is_live(), "the whole old graph is reclaimed");
    let current = rt.atom_load(&atom).unwrap();
    assert_eq!(items(&rt, &current), [ValueView::Number(8.0)]);
}

#[test]
fn a_task_keeps_a_loaded_value_across_a_yield_replacement_and_collection() {
    let rt = Runtime::new();
    let initial = vector(&rt, &[1.0, 2.0, 3.0]);
    let atom = rt.new_atom(&initial).unwrap();
    let old_weak = initial.downgrade();
    drop(initial);
    // let held = atom_load(arg0); /* yield */ return held
    let reader = program("reader", 1, 2, |asm| {
        asm.get_local(0).op(Opcode::AtomLoad).set_local(1);
        asm.get_local(1).op(Opcode::Return);
    });
    let task = rt.spawn(&reader, &[&atom]).unwrap();
    let first = task
        .run_with(Budget::instructions(3), &mut NoEffects)
        .unwrap();
    assert!(matches!(first, RunOutcome::Yielded), "{first:?}");

    thread::scope(|t| {
        t.spawn(|| {
            let old = rt.atom_load(&atom).unwrap();
            swapped(rt.atom_compare_and_set(&atom, &old, &rt.root(num(0.0))));
        })
        .join()
        .unwrap();
    });
    rt.collect().unwrap();
    assert!(old_weak.is_live(), "the suspended frame roots it");

    let result = thread::scope(|t| t.spawn(|| complete(task.run().unwrap())).join().unwrap());
    assert!(result.same_object(&old_weak.upgrade().unwrap()));
    assert_eq!(items(&rt, &result).len(), 3);
    drop((result, task));
    rt.collect().unwrap();
    assert!(
        !old_weak.is_live(),
        "reclaimable once the task and result are gone"
    );
}

/// Counts host effects; each returns a fresh number.
#[derive(Default)]
struct EffectCounter {
    calls: usize,
}

impl Host for EffectCounter {
    fn effect(&mut self, _call: HostCall) -> Result<Primitive, String> {
        self.calls += 1;
        Ok(num(100.0 + self.calls as f64))
    }
}

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn a_forced_cas_conflict_does_not_repeat_the_precomputed_effect() {
    let rt = Runtime::new();
    let atom = rt.new_atom(&rt.root(num(0.0))).unwrap();
    // let old = atom_load(a); let new = effect(old); /* yield */
    // return [atom_cas(a, old, new)...]  (witness, ok)
    let updater = program("updater", 1, 3, |asm| {
        asm.get_local(0).op(Opcode::AtomLoad).set_local(1);
        asm.get_local(1).host_effect(0).set_local(2);
        asm.get_local(0)
            .get_local(1)
            .get_local(2)
            .op(Opcode::AtomCas);
        asm.new_vector(2).op(Opcode::Return);
    });
    // A second writer task: atom_cas(a, atom_load(a), 7)
    let interloper = program("interloper", 1, 1, |asm| {
        asm.get_local(0).get_local(0).op(Opcode::AtomLoad);
        asm.constant(num(7.0)).op(Opcode::AtomCas);
        asm.new_vector(2).op(Opcode::Return);
    });

    let mut host = EffectCounter::default();
    let task = rt.spawn(&updater, &[&atom]).unwrap();
    let first = task.run_with(Budget::instructions(6), &mut host).unwrap();
    assert!(matches!(first, RunOutcome::Yielded), "{first:?}");
    assert_eq!(
        host.calls, 1,
        "the new value was computed once, before the CAS"
    );

    // Force the conflict: the other writer runs to completion on another
    // worker between the updater's computation and its CAS.
    let other = thread::scope(|t| {
        t.spawn(|| {
            let task = rt.spawn(&interloper, &[&atom]).unwrap();
            complete(task.run().unwrap())
        })
        .join()
        .unwrap()
    });
    assert_eq!(
        items(&rt, &other),
        [ValueView::Number(0.0), ValueView::Bool(true)]
    );

    let result = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(
        items(&rt, &result),
        [ValueView::Number(7.0), ValueView::Bool(false)],
        "the conflict reports the current value"
    );
    assert_eq!(host.calls, 1, "the effect was not repeated");
    assert_eq!(rt.atom_load(&atom).unwrap().view(), ValueView::Number(7.0));

    // Without interference the same program installs its one computed value.
    let task = rt.spawn(&updater, &[&atom]).unwrap();
    let result = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(
        items(&rt, &result),
        [ValueView::Number(7.0), ValueView::Bool(true)]
    );
    assert_eq!(host.calls, 2);
    assert_eq!(
        rt.atom_load(&atom).unwrap().view(),
        ValueView::Number(102.0)
    );
}

#[test]
fn readers_see_fully_initialized_published_graphs() {
    const ROUNDS: usize = 200;
    const WIDTH: usize = 32;
    let rt = Runtime::new();
    let atom = rt.new_atom(&rt.root(Primitive::Null)).unwrap();
    thread::scope(|t| {
        t.spawn(|| {
            let mut old = rt.root(Primitive::Null);
            for round in 1..=ROUNDS {
                // [{0: round, 1: i} for i in 0..WIDTH], built then published.
                let new = rt.scope(|s| {
                    let maps: Vec<_> = (0..WIDTH)
                        .map(|i| {
                            let entries = [
                                (s.primitive(num(0.0)), s.primitive(num(round as f64))),
                                (s.primitive(num(1.0)), s.primitive(num(i as f64))),
                            ];
                            s.new_map(&entries).unwrap()
                        })
                        .collect();
                    let v = s.new_vector(&maps).unwrap();
                    s.persist(v)
                });
                swapped(rt.atom_compare_and_set(&atom, &old, &new));
                old = new;
            }
        });
        t.spawn(|| {
            let deadline = Instant::now() + HANDSHAKE;
            let mut last = 0;
            while last < ROUNDS {
                assert!(Instant::now() < deadline, "writer did not finish");
                let seen = rt.atom_load(&atom).unwrap();
                if seen.view() == ValueView::Null {
                    continue;
                }
                rt.scope(|s| {
                    let v = s.local(&seen).unwrap();
                    assert_eq!(s.count(v).unwrap(), WIDTH);
                    let round_of = |m| {
                        let key = s.primitive(num(0.0));
                        s.view(s.get(m, key).unwrap().unwrap())
                    };
                    let ValueView::Number(round) = round_of(s.nth(v, 0).unwrap()) else {
                        panic!("bad round");
                    };
                    for i in 0..WIDTH {
                        let m = s.nth(v, i).unwrap();
                        assert_eq!(round_of(m), ValueView::Number(round));
                        let key = s.primitive(num(1.0));
                        let index = s.view(s.get(m, key).unwrap().unwrap());
                        assert_eq!(index, ValueView::Number(i as f64));
                    }
                    assert!(round as usize >= last, "rounds never go backwards");
                    last = round as usize;
                });
            }
        });
    });
}

#[test]
fn every_installing_operation_rejects_task_owned_mutation() {
    let rt = Runtime::new();
    let object = plain_object(&rt);
    let not_shareable = |e: AtomError, path: &[PathStep]| match e {
        AtomError::NotShareable(PublishError::NotShareable { path: p, found, .. }) => {
            assert_eq!(p.steps(), path);
            assert_eq!(found, "object");
        }
        other => panic!("expected NotShareable, got {other:?}"),
    };
    not_shareable(rt.new_atom(&object).unwrap_err(), &[]);
    let holder = rt.scope(|s| {
        let one = s.primitive(num(1.0));
        let o = s.local(&object).unwrap();
        let v = s.new_vector(&[one, o]).unwrap();
        s.persist(v)
    });
    not_shareable(rt.new_atom(&holder).unwrap_err(), &[PathStep::Index(1)]);
    not_shareable(
        rt.new_single_writer_atom(&holder).unwrap_err(),
        &[PathStep::Index(1)],
    );

    let initial = rt.root(num(1.0));
    let atom = rt.new_atom(&initial).unwrap();
    not_shareable(
        rt.atom_compare_and_set(&atom, &initial, &holder)
            .unwrap_err(),
        &[PathStep::Index(1)],
    );
    assert_eq!(rt.atom_load(&atom).unwrap().view(), ValueView::Number(1.0));

    // The VM intrinsics apply the same check: a plain object, a builder.
    let new_atom = program("new-atom", 0, 0, |asm| {
        asm.op(Opcode::NewObject)
            .op(Opcode::AtomNew)
            .op(Opcode::Return);
    });
    let kind = failed_kind(rt.spawn(&new_atom, &[]).unwrap().run().unwrap());
    assert_eq!(kind, VmErrorKind::Atom(AtomErrorKind::NotShareable));
    let cas_builder = program("cas-builder", 2, 2, |asm| {
        asm.get_local(0).get_local(1).op(Opcode::NewVectorBuilder);
        asm.op(Opcode::AtomCas).op(Opcode::Return);
    });
    let task = rt.spawn(&cas_builder, &[&atom, &initial]).unwrap();
    let RunOutcome::Failed(err) = task.run().unwrap() else {
        panic!("expected Failed")
    };
    assert_eq!(err.kind(), VmErrorKind::Atom(AtomErrorKind::NotShareable));
    assert!(
        err.message().contains("vector builder"),
        "{}",
        err.message()
    );
    assert_eq!(rt.atom_load(&atom).unwrap().view(), ValueView::Number(1.0));

    // Operands that are not atoms are rejected by kind.
    let load = program("load", 1, 1, |asm| {
        asm.get_local(0).op(Opcode::AtomLoad).op(Opcode::Return);
    });
    let kind = failed_kind(rt.spawn(&load, &[&holder]).unwrap().run().unwrap());
    assert_eq!(
        kind,
        VmErrorKind::Atom(AtomErrorKind::NotAnAtom { found: "vector" })
    );
    assert_eq!(
        rt.atom_load(&initial).unwrap_err(),
        AtomError::NotAnAtom { found: "number" }
    );

    // An atom is an explicit concurrent reference: publishable, also inside
    // persistent values and inside another atom.
    let shelf = rt.scope(|s| {
        let a = s.local(&atom).unwrap();
        let v = s.new_vector(&[a]).unwrap();
        s.persist(v)
    });
    rt.publish(&shelf).unwrap();
    let outer = rt.new_atom(&shelf).unwrap();
    assert!(rt.atom_load(&outer).unwrap().same_object(&shelf));
}

#[test]
fn single_writer_publication_has_its_own_rules() {
    let rt = Runtime::new();
    let mut publisher = rt.new_single_writer_atom(&rt.root(num(0.0))).unwrap();
    let atom = publisher.atom().clone();

    // Any number of publications, each replacing the last (unlike a
    // dependency cell's single assignment); readers see the latest.
    for n in 1..=3 {
        publisher.publish(&vector(&rt, &[f64::from(n)])).unwrap();
        let seen = rt.atom_load(&atom).unwrap();
        assert_eq!(items(&rt, &seen), [ValueView::Number(f64::from(n))]);
    }

    // No one else can write: CAS is rejected from Rust and from the VM,
    // even with the right expected value.
    let current = rt.atom_load(&atom).unwrap();
    assert_eq!(
        rt.atom_compare_and_set(&atom, &current, &rt.root(num(9.0)))
            .unwrap_err(),
        AtomError::SingleWriter
    );
    let cas = program("cas", 2, 2, |asm| {
        asm.get_local(0).get_local(1).constant(num(9.0));
        asm.op(Opcode::AtomCas).op(Opcode::Return);
    });
    let kind = failed_kind(rt.spawn(&cas, &[&atom, &current]).unwrap().run().unwrap());
    assert_eq!(kind, VmErrorKind::Atom(AtomErrorKind::SingleWriter));
    assert!(rt.atom_load(&atom).unwrap().same_object(&current));

    // Tasks read it like any atom.
    let load = program("load", 1, 1, |asm| {
        asm.get_local(0).op(Opcode::AtomLoad).op(Opcode::Return);
    });
    let seen = complete(rt.spawn(&load, &[&atom]).unwrap().run().unwrap());
    assert!(seen.same_object(&current));

    // Rejected publications leave the atom and the publisher intact.
    assert!(matches!(
        publisher.publish(&plain_object(&rt)),
        Err(AtomError::NotShareable(_))
    ));
    let foreign = Runtime::new().root(num(1.0));
    assert_eq!(
        publisher.publish(&foreign).unwrap_err(),
        AtomError::Handle(HandleError::ForeignRuntime)
    );
    assert!(rt.atom_load(&atom).unwrap().same_object(&current));

    // The publisher is a capability that can move to another worker.
    let rt_ref = &rt;
    let published = thread::scope(|t| {
        t.spawn(move || {
            let v = vector(rt_ref, &[4.0]);
            publisher.publish(&v).unwrap();
            v
        })
        .join()
        .unwrap()
    });
    assert!(rt.atom_load(&atom).unwrap().same_object(&published));
}

#[test]
fn atom_contents_are_traced_and_dead_atoms_are_reclaimed() {
    let rt = Runtime::new();
    let content = vector(&rt, &[1.0]);
    let content_weak = content.downgrade();
    let atom = rt.new_atom(&content).unwrap();
    drop(content);
    for _ in 0..2 {
        rt.collect().unwrap();
        assert!(content_weak.is_live(), "reachable through the rooted atom");
    }
    assert_eq!(rt.heap_stats().objects, 2);

    // Old contents become garbage once replaced and released.
    let old = rt.atom_load(&atom).unwrap();
    swapped(rt.atom_compare_and_set(&atom, &old, &vector(&rt, &[2.0])));
    drop(old);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!content_weak.is_live());

    // A dead atom's handle is stale: it never names the slot's next object.
    let atom_weak = atom.downgrade();
    drop(atom);
    assert_eq!(rt.collect().unwrap().reclaimed, 2, "the atom and its value");
    let fresh = rt.new_atom(&rt.root(num(5.0))).unwrap();
    assert_eq!(atom_weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    assert_eq!(rt.atom_load(&fresh).unwrap().view(), ValueView::Number(5.0));
}
