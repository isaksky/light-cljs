//! Private task-owned storage, independent of the compiler's getter protocol.
use big_boy_pants_js::frontend::{compile_function, compile_script};
use big_boy_pants_js::{
    Budget, HeapPolicy, Host, Rooted, RunOutcome, Runtime, ValueView, VmErrorKind, VmLimits,
};

struct NoEffects;
impl Host for NoEffects {
    fn effect(
        &mut self,
        _: big_boy_pants_js::HostCall,
    ) -> Result<big_boy_pants_js::Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(value) => value,
        other => panic!("expected completion: {other:?}"),
    }
}

fn runtime(bytes: usize) -> Runtime {
    Runtime::with_heap_policy(
        VmLimits::default(),
        HeapPolicy {
            hard_limit_bytes: Some(bytes),
            ..Default::default()
        },
    )
}

#[test]
fn slots_trace_full_identities_across_collection_and_worker_migration() {
    let rt = runtime(16 << 20);
    let code = compile_script(
        "slots",
        r#"
        let unit = {u: 7}, name = {n: 8};
        const slot = __rt.initializerSlot(unit, name);
        unit = null; name = null;
        __rt.initializerSlotSet(slot, 3, {slot, count: 9});
        __rt.initializerSlotSet(slot, 2, 1);
        let n = 0; while (n < 100) n++;
        if (__rt.initializerSlotGet(slot, 0).u !== 7 ||
            __rt.initializerSlotGet(slot, 1).n !== 8 ||
            __rt.initializerSlotGet(slot, 2) !== 1 ||
            __rt.initializerSlotGet(slot, 3).slot !== slot ||
            __rt.initializerSlotGet(slot, 3).count !== 9) throw 'lost edge';
        for (const unit of [1.5, -0, NaN, undefined, null, 'unit', Symbol('unit'), {}, () => 1]) {
            const saved = __rt.initializerSlot(unit, unit);
            if (!Object.is(__rt.initializerSlotGet(saved, 0), unit) ||
                !Object.is(__rt.initializerSlotGet(saved, 1), unit)) throw 'narrowed identity';
        }
        for (const value of [undefined, null, false, 0, -0, NaN, '', Symbol('x'), {x: 1}]) {
            __rt.initializerSlotSet(slot, 3, value);
            __rt.initializerSlotSet(slot, 2, 3);
            if (!Object.is(__rt.initializerSlotGet(slot, 3), value)) throw 'lost value';
        }
        __rt.initializerSlotSet(slot, 4, true);
        __rt.initializerSlotGet(slot, 4) === true
        "#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let mut yields = 0;
    let result = loop {
        // Each slice moves the same saved task to a fresh OS worker.
        let outcome = std::thread::scope(|s| {
            s.spawn(|| task.run_with(Budget::instructions(31), &mut NoEffects))
                .join()
                .unwrap()
                .unwrap()
        });
        if matches!(outcome, RunOutcome::Yielded) {
            yields += 1;
            assert!(yields < 1000);
            rt.collect().unwrap();
        } else {
            break complete(outcome);
        }
    };
    assert!(yields > 20);
    assert_eq!(result.view(), ValueView::Bool(true));
    drop((task, result));
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.native, 0, "slot cycle leaked");
}

#[test]
fn slots_reject_publication_and_foreign_reads_and_writes() {
    let rt = Runtime::new();
    let make = compile_script("slot", "__rt.initializerSlot(1.5, 'name')").unwrap();
    let slot = complete(rt.spawn(&make, &[]).unwrap().run().unwrap());
    assert!(rt.publish(&slot).is_err());
    for source in [
        "function f(s) { return __rt.initializerSlotGet(s, 0); }",
        "function f(s) { return __rt.initializerSlotSet(s, 3, 7); }",
    ] {
        let code = compile_function(source).unwrap();
        let RunOutcome::Failed(error) = rt.spawn(&code, &[&slot]).unwrap().run().unwrap() else {
            panic!("foreign slot operation succeeded")
        };
        assert!(matches!(error.kind(), VmErrorKind::ForeignObject { .. }));
    }
}

#[test]
fn slot_bodies_are_charged_to_the_aggregate_byte_limit() {
    let rt = runtime(2 << 20);
    let code = compile_script(
        "slot limit",
        "const slots = []; for (let i = 0; i < 100000; i++) slots.push(__rt.initializerSlot(i, null));",
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Failed(error) = task.run().unwrap() else {
        panic!("slots exceeded the aggregate limit")
    };
    assert!(matches!(error.kind(), VmErrorKind::HeapLimit));
    assert!(rt.heap_stats().peak_managed_bytes <= 2 << 20);
    drop(task);
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.native, 0);
    let code = compile_script(
        "recover",
        "__rt.initializerSlotGet(__rt.initializerSlot(1.5, null), 0) === 1.5",
    )
    .unwrap();
    assert_eq!(
        complete(rt.spawn(&code, &[]).unwrap().run().unwrap()).view(),
        ValueView::Bool(true)
    );
}

#[test]
fn fixed_slots_retain_less_storage_than_property_bags() {
    let retained = |constructor: &str| {
        let rt = runtime(64 << 20);
        let code = compile_script(
            "storage",
            &format!(
                "const slots = []; for (let i = 0; i < 1200; i++) slots.push({constructor}); slots"
            ),
        )
        .unwrap();
        let slots = complete(rt.spawn(&code, &[]).unwrap().run().unwrap());
        rt.collect().unwrap();
        let stats = rt.heap_stats();
        eprintln!("{constructor}: {stats:?}; {:?}", rt.heap_usage());
        drop(slots);
        stats.last_live_bytes
    };
    let bags = retained("{unit:i, name:null, state:0, value:null}");
    let arrays = retained("[i,null,0,null,false]");
    let slots = retained("__rt.initializerSlot(i,null)");
    assert!(
        slots < arrays && arrays < bags,
        "bags/arrays/slots: {bags}/{arrays}/{slots}"
    );
}
