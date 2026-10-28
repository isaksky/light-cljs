//! Synthetic JavaScript fixtures, not light-cljs output.
use big_boy_pants_js::host::{TimeZone, sim::SimTimeHost};
use big_boy_pants_js::{Budget, Host, RunOutcome, Runtime, ValueView, frontend::compile_script};

fn evaluate(runtime: &Runtime, host: &mut dyn Host, source: &str) -> ValueView {
    let task = runtime
        .spawn(&compile_script("date-host.js", source).unwrap(), &[])
        .unwrap();
    for _ in 0..10000 {
        match task.run_with(Budget::instructions(1), host).unwrap() {
            RunOutcome::Complete(value) => {
                assert!(task.dependency_log().edges().is_empty());
                return runtime.scope(|s| s.view(s.local(&value).unwrap()));
            }
            RunOutcome::Yielded => {}
            other => panic!("{source}: {other:?}"),
        }
    }
    panic!("bounded date evaluation did not complete");
}

#[test]
fn date_host_clock_and_zone() {
    let runtime = Runtime::new();
    let mut host = SimTimeHost::new(
        1506098258091.0,
        TimeZone::fixed_offset_seconds(3600).unwrap(),
    );
    assert_eq!(
        evaluate(
            &runtime,
            &mut host,
            "Date.now() === 1506098258091 && new Date().getTime() === 1506098258091 && new Date(2000, 0).getTime() === 946681200000 && new Date(2000, 0).getTimezoneOffset() === -60"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(host.clock_reads(), 2);
    let mut utc = SimTimeHost::new(0.0, TimeZone::UTC);
    // All tasks/worlds in this runtime keep the first host's zone.
    assert_eq!(
        evaluate(&runtime, &mut utc, "new Date(2000, 0).getTimezoneOffset()"),
        ValueView::Number(-60.0)
    );
    assert_eq!(
        evaluate(&Runtime::new(), &mut utc, "new Date(2000, 0).getTime()"),
        ValueView::Number(946684800000.0)
    );
    let mut paris = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    assert_eq!(
        evaluate(
            &Runtime::new(),
            &mut paris,
            "new Date(2020, 0).getTimezoneOffset() === -60 && new Date(2020, 6).getTimezoneOffset() === -120 && new Date(2020, 2, 29, 2, 30).getTime() === Date.UTC(2020, 2, 29, 1, 30) && new Date(2020, 9, 25, 2, 30).getTime() === Date.UTC(2020, 9, 25, 0, 30)"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn textual_instant_literals_select_the_host_zone_before_canonicalizing() {
    for zone in [
        TimeZone::UTC,
        TimeZone::fixed_offset_seconds(3600).unwrap(),
        TimeZone::named("Europe/Paris").unwrap(),
    ] {
        for text in [
            "2024-01-02T03:04:05",
            "2024-07-02T03:04:05",
            "2024-01-02T03:04:05Z",
            "2024-01-02T03:04:05+01:00",
        ] {
            let runtime = Runtime::new();
            let mut host = SimTimeHost::new(0.0, zone);
            let source = format!(
                r#"const text = '{text}';
                const a = __rt.formCompilerLiteral('instant', text);
                const time = Date.parse(text);
                const b = __rt.formCompilerLiteral('instant', text);
                const c = __rt.formCompilerLiteral('instant', time);
                let map = __rt.formMap();
                for (let i = 0; i < 20; i++) map = __rt.formAssoc(map, i, i);
                map = __rt.formAssoc(map, a, 'found');
                __rt.formLiteralPayload(a) === text &&
                __rt.formEquals(a, b) && __rt.formEquals(a, c) &&
                __rt.formHash(a) === __rt.formHash(b) &&
                __rt.formHash(a) === __rt.formHash(c) &&
                __rt.formGet(map, b, null) === 'found' &&
                __rt.formGet(map, c, null) === 'found';"#
            );
            assert_eq!(
                evaluate(&runtime, &mut host, &source),
                ValueView::Bool(true),
                "{zone:?}, {text}"
            );
        }
    }
}

#[test]
fn hostless_textual_instant_literals_freeze_the_runtime_utc_default() {
    let runtime = Runtime::new();
    let literal = runtime.scope(|s| {
        let payload = s.primitive(big_boy_pants_js::Primitive::String(
            "2024-01-02T03:04:05".into(),
        ));
        let literal = s.new_compiler_literal("instant".into(), payload).unwrap();
        s.persist(literal)
    });
    runtime.publish(&literal).unwrap();
    runtime.collect().unwrap();
    let mut host = SimTimeHost::new(0.0, TimeZone::fixed_offset_seconds(3600).unwrap());
    assert_eq!(
        evaluate(
            &runtime,
            &mut host,
            "Date.parse('2024-01-02T03:04:05') === 1704164645000"
        ),
        ValueView::Bool(true)
    );
    runtime.scope(|s| {
        let payload = s.primitive(big_boy_pants_js::Primitive::String(
            "2024-01-02T03:04:05".into(),
        ));
        let again = s.new_compiler_literal("instant".into(), payload).unwrap();
        let literal = s.local(&literal).unwrap();
        assert!(s.form_equals(literal, again));
        assert_eq!(s.form_hash(literal), s.form_hash(again));
    });
}

#[test]
fn date_string_named_zone_formats() {
    let runtime = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    for source in [
        "new Date(Date.UTC(2020, 0, 1)).toString() === 'Wed Jan 01 2020 01:00:00 GMT+0100'",
        "new Date(Date.UTC(2020, 6, 1)).toString() === 'Wed Jul 01 2020 02:00:00 GMT+0200'",
        "new Date(Date.UTC(2020, 2, 29, 0, 59, 59)).toTimeString() === '01:59:59 GMT+0100' && new Date(Date.UTC(2020, 2, 29, 1)).toTimeString() === '03:00:00 GMT+0200'",
        "new Date(8.64e15).toString() === 'Sat Sep 13 275760 01:00:00 GMT+0100'",
    ] {
        assert_eq!(
            evaluate(&runtime, &mut host, source),
            ValueView::Bool(true),
            "{source}"
        );
    }
    let mut utc = SimTimeHost::new(0.0, TimeZone::UTC);
    assert_eq!(
        evaluate(
            &runtime,
            &mut utc,
            "Date() === 'Thu Jan 01 1970 01:00:00 GMT+0100'"
        ),
        ValueView::Bool(true)
    );
    let mut monrovia = SimTimeHost::new(0.0, TimeZone::named("Africa/Monrovia").unwrap());
    assert_eq!(
        evaluate(
            &Runtime::new(),
            &mut monrovia,
            "new Date(Date.UTC(1900, 0)).toString() === 'Sun Dec 31 1899 23:17:00 GMT-0043'"
        ),
        ValueView::Bool(true)
    );
    // QuickJS truncates negative epoch milliseconds toward zero to select the
    // offset's whole second, including the final millisecond before a change.
    assert_eq!(
        evaluate(
            &Runtime::new(),
            &mut monrovia,
            "new Date(-1604359013000).toTimeString().slice(-8) === 'GMT-0043' && new Date(-1604359012001).toTimeString().slice(-8) === 'GMT-0044' && new Date(-1604359012000).toTimeString().slice(-8) === 'GMT-0044'"
        ),
        ValueView::Bool(true)
    );
    for time in [f64::NAN, f64::INFINITY, 8.64e15 + 1.0] {
        let mut host = SimTimeHost::new(time, TimeZone::UTC);
        assert_eq!(
            evaluate(&Runtime::new(), &mut host, "Date() === 'Invalid Date'"),
            ValueView::Bool(true)
        );
        assert_eq!(host.clock_reads(), 1);
    }
}

#[test]
fn date_without_host_time_inputs() {
    let runtime = Runtime::new();
    let task = runtime.spawn(&compile_script("date-host.js", "let caught = 0; try { Date.now(); } catch(e) { if(e instanceof RangeError) caught++; } try { new Date(); } catch(e) { if(e instanceof RangeError) caught++; } caught === 2 && new Date(2000, 0).getTime() === 946684800000 && new Date(0).getTimezoneOffset() === 0").unwrap(), &[]).unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("expected completion")
    };
    assert_eq!(
        runtime.scope(|s| s.view(s.local(&value).unwrap())),
        ValueView::Bool(true)
    );
}

#[test]
fn date_local_fields_and_setters() {
    let mut host = SimTimeHost::new(0.0, TimeZone::fixed_offset_seconds(3600).unwrap());
    for source in [
        "let d = new Date(2017, 8, 22, 18, 10, 11, 91); d.getFullYear() === 2017 && d.getMonth() === 8 && d.getDate() === 22 && d.getHours() === 18 && d.getMinutes() === 10 && d.getSeconds() === 11 && d.getMilliseconds() === 91 && d.getDay() === 5 && d.getUTCHours() === 17",
        "let d = new Date(2000, 0); d.setFullYear(2001, 1, 2); d.setMonth(2, 3); d.setDate(4); d.setHours(5, 6, 7, 8); d.setMinutes(9, 10, 11); d.setSeconds(12, 13); d.setMilliseconds(14); d.toISOString() === '2001-03-04T04:09:12.014Z'",
        "let d = new Date(NaN); let nan = d.setHours(3); Number.isNaN(nan) && Number.isNaN(d.getTimezoneOffset()) && Number.isNaN(d.getFullYear()) && d.setFullYear(2000) === Date.UTC(1999, 11, 31, 23)",
        "let d = new Date(0); let n = d.setHours({valueOf() {d.setTime(86400000); return 2;}}); n === 3600000 && d.getTime() === 3600000",
        "let log = ''; let d = new Date({valueOf() {log += 'y'; return 99;}}, {valueOf() {log += 'm'; return 0;}}, {valueOf() {log += 'd'; return 1;}}, 0, 0, 0, 0, Symbol()); log === 'ymd' && d.getFullYear() === 1999",
        "let log = ''; let d = new Date(NaN); d.setHours({valueOf() {log += 'h'; return NaN;}}, {valueOf() {log += 'm'; return 1;}}); log === 'hm' && Number.isNaN(d.getTime())",
        "let called = false; let caught = false; try {Date.prototype.setHours.call({}, {valueOf() {called = true; return 1;}});} catch(e) {caught = e instanceof TypeError;} caught && !called",
        "let d = Object.getOwnPropertyDescriptor(Date, 'now'); Date.now.name === 'now' && Date.now.length === 0 && d.writable && !d.enumerable && d.configurable && Date.prototype.setHours.length === 4 && Date.prototype.getHours.length === 0",
        "let d = new Date(8.64e15); d.getHours() === 1 && d.setHours(1) === 8.64e15 && new Date(275760, 8, 13, 1).getTime() === 8.64e15",
    ] {
        assert_eq!(
            evaluate(&Runtime::new(), &mut host, source),
            ValueView::Bool(true),
            "{source}"
        );
    }
    let mut paris = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    assert_eq!(
        evaluate(
            &Runtime::new(),
            &mut paris,
            "let d = new Date(2020, 2, 29, 0, 30); d.setHours(2); d.getTime() === Date.UTC(2020, 2, 29, 1, 30) && d.getHours() === 3"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn date_rule_table_coverage_and_seconds() {
    assert!(TimeZone::named("not/a/zone").is_err());
    assert!(TimeZone::named("Africa/Abidjan").is_err()); // Inventory-only name.
    assert!(TimeZone::fixed_offset_seconds(86400).is_err());
    let monrovia = TimeZone::named("Africa/Monrovia").unwrap();
    assert_eq!(monrovia.offset_seconds(-8.64e15), -2588);
    assert_eq!(monrovia.local_time_millis(-8.64e15), -8.64e15 - 2588000.0);
    assert_eq!(monrovia.utc_time_millis(-8.64e15 - 2588000.0), -8.64e15);
    assert_eq!(monrovia.offset_seconds(8.64e15), 0);
    let paris = TimeZone::named("Europe/Paris").unwrap();
    assert_eq!(paris.offset_seconds(-8.64e15), 561);
    // No extrapolation of future DST recurrence beyond the snapshot.
    assert_eq!(paris.offset_seconds(8.64e15), 3600);
    assert_eq!(
        TimeZone::named("US/Eastern").unwrap(),
        TimeZone::named("America/New_York").unwrap()
    );
    let mut host = SimTimeHost::new(0.0, monrovia);
    assert_eq!(
        evaluate(
            &Runtime::new(),
            &mut host,
            "let d = new Date(Date.UTC(1900, 0)); d.getTimezoneOffset() === 2588 / 60 && d.getSeconds() === 52"
        ),
        ValueView::Bool(true)
    );
}

#[test]
fn date_local_coercion_waits_collects_and_migrates() {
    use big_boy_pants_js::{Completion, Primitive};
    let runtime = Runtime::new();
    let source = "let log = ''; let d = new Date({valueOf() {log += 'y'; __rt.need(1, 1); return 2020;}}, 2, 29, 0, 30); let result = d.setHours({get valueOf() {log += 'g'; __rt.need(2, 1); return function() {log += 'h'; __rt.need(3, 1); return 2;};}}, {valueOf() {log += 'm'; __rt.need(4, 1); return 30;}}); result === Date.UTC(2020, 2, 29, 1, 30) && d.getHours() === 3 && log === 'yghm'";
    let mut task = runtime
        .spawn(&compile_script("date-wait.js", source).unwrap(), &[])
        .unwrap();
    let mut waits = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let mut host = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
            let outcome = task.run_with(Budget::instructions(1), &mut host).unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waits += 1;
                assert!(waits <= 4);
                runtime.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = runtime.root(Primitive::Number(0.0));
                task = runtime
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waits, 4);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("bounded date migration run did not complete");
}

#[test]
fn date_time_host_calls_are_leaf_and_unrecorded() {
    use big_boy_pants_js::{HostCall, Primitive, RunError};
    struct GuardHost {
        runtime: Runtime,
        reads: usize,
        zones: usize,
    }
    impl GuardHost {
        fn check(&self) {
            let task = self
                .runtime
                .spawn(&compile_script("leaf.js", "0").unwrap(), &[])
                .unwrap();
            assert!(matches!(task.run(), Err(RunError::InsideHostEffect)));
        }
    }
    impl Host for GuardHost {
        fn current_time_millis(&mut self) -> Result<f64, String> {
            self.check();
            self.reads += 1;
            Ok(self.reads as f64)
        }
        fn default_time_zone(&mut self) -> TimeZone {
            self.check();
            self.zones += 1;
            TimeZone::UTC
        }
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            unreachable!()
        }
    }
    let runtime = Runtime::new();
    let mut host = GuardHost {
        runtime: runtime.clone(),
        reads: 0,
        zones: 0,
    };
    assert_eq!(
        evaluate(
            &runtime,
            &mut host,
            "Date.now() === 1 && new Date().getTime() === 2 && new Date(2000, 0).getHours() === 0"
        ),
        ValueView::Bool(true)
    );
    assert_eq!(host.zones, 1);
    assert_eq!(
        evaluate(&runtime, &mut host, "Date.now()"),
        ValueView::Number(3.0)
    );
}

#[test]
fn date_host_clock_time_clip() {
    for (input, expected) in [
        (1.9, 1.0),
        (-1.9, -1.0),
        (-0.9, 0.0),
        (-0.0, 0.0),
        (8.64e15, 8.64e15),
        (-8.64e15, -8.64e15),
        (8.64e15 + 1.0, f64::NAN),
        (-8.64e15 - 1.0, f64::NAN),
        (f64::NAN, f64::NAN),
        (f64::INFINITY, f64::NAN),
        (f64::NEG_INFINITY, f64::NAN),
    ] {
        let runtime = Runtime::new();
        let mut host = SimTimeHost::new(input, TimeZone::UTC);
        for source in ["Date.now()", "new Date().getTime()"] {
            let ValueView::Number(actual) = evaluate(&runtime, &mut host, source) else {
                panic!("{source}: expected number");
            };
            if expected.is_nan() {
                assert!(actual.is_nan(), "{source} for {input}: {actual}");
            } else {
                assert_eq!(actual.to_bits(), expected.to_bits(), "{source} for {input}");
            }
        }
        assert_eq!(host.clock_reads(), 2);
    }
}

#[test]
fn date_zone_transition_boundaries() {
    let runtime = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    // Each transition instant belongs to the new offset interval.
    for (transition, before, after) in [
        (1585443600000_i64, -60.0, -120.0),
        (1603587600000_i64, -120.0, -60.0),
    ] {
        for (delta, expected) in [(-1, before), (0, after), (1, after)] {
            let source = format!("new Date({}).getTimezoneOffset()", transition + delta);
            assert_eq!(
                evaluate(&runtime, &mut host, &source),
                ValueView::Number(expected),
                "{source}"
            );
        }
    }
    // Gap [02:00,03:00): use the earlier offset inside the gap, and the new
    // offset at its excluded upper endpoint. Overlap [02:00,03:00): choose
    // the earlier UTC instant; 03:00 belongs only to the later interval.
    for (fields, expected) in [
        ("2020, 2, 29, 1, 59, 59, 999", 1585443599999_i64),
        ("2020, 2, 29, 2, 0, 0, 0", 1585443600000),
        ("2020, 2, 29, 2, 0, 0, 1", 1585443600001),
        ("2020, 2, 29, 2, 59, 59, 999", 1585447199999),
        ("2020, 2, 29, 3, 0, 0, 0", 1585443600000),
        ("2020, 2, 29, 3, 0, 0, 1", 1585443600001),
        ("2020, 9, 25, 1, 59, 59, 999", 1603583999999),
        ("2020, 9, 25, 2, 0, 0, 0", 1603584000000),
        ("2020, 9, 25, 2, 0, 0, 1", 1603584000001),
        ("2020, 9, 25, 2, 59, 59, 999", 1603587599999),
        ("2020, 9, 25, 3, 0, 0, 0", 1603591200000),
        ("2020, 9, 25, 3, 0, 0, 1", 1603591200001),
    ] {
        let source = format!("new Date({fields}).getTime()");
        assert_eq!(
            evaluate(&runtime, &mut host, &source),
            ValueView::Number(expected as f64),
            "{source}"
        );
    }
}

#[test]
fn date_iso_local_zone_disambiguation() {
    let rt = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    for source in [
        "Date.parse('2020-03-29T02:30') === Date.UTC(2020, 2, 29, 1, 30)",
        "new Date('2020-10-25T02:30').getTime() === Date.UTC(2020, 9, 25, 0, 30)",
        "Date.parse('2020-03-29') === Date.UTC(2020, 2, 29) && Date.parse('2020-10-25T02:30+01:00') === Date.UTC(2020, 9, 25, 1, 30)",
    ] {
        assert_eq!(
            evaluate(&rt, &mut host, source),
            ValueView::Bool(true),
            "{source}"
        );
    }
    let mut utc = SimTimeHost::new(0.0, TimeZone::UTC);
    assert_eq!(
        evaluate(&rt, &mut utc, "Date.parse('2000T00:00')"),
        ValueView::Number(946681200000.0)
    );
    // Explicit UTC parsing leaves the first-local-operation zone selection free.
    let rt = Runtime::new();
    assert_eq!(
        evaluate(&rt, &mut utc, "Date.parse('2000')"),
        ValueView::Number(946684800000.0)
    );
    assert_eq!(
        evaluate(&rt, &mut host, "Date.parse('2000T00:00')"),
        ValueView::Number(946681200000.0)
    );
}

#[test]
fn date_fallback_local_zone_disambiguation() {
    let rt = Runtime::new();
    let mut host = SimTimeHost::new(0.0, TimeZone::named("Europe/Paris").unwrap());
    for source in [
        "Date.parse('Mar 29 2020 2:30') === Date.UTC(2020, 2, 29, 1, 30)",
        "new Date('Oct 25 2020 2:30').getTime() === Date.UTC(2020, 9, 25, 0, 30)",
        "Date.parse('Mar 29 2020') === Date.UTC(2020, 2, 28, 23)",
        "Date.parse('Oct 25 2020 2:30 GMT+0100') === Date.UTC(2020, 9, 25, 1, 30)",
    ] {
        assert_eq!(
            evaluate(&rt, &mut host, source),
            ValueView::Bool(true),
            "{source}"
        );
    }
    // Explicit-zone fallback parsing must not freeze the runtime's local zone.
    let rt = Runtime::new();
    let mut utc = SimTimeHost::new(0.0, TimeZone::UTC);
    assert_eq!(
        evaluate(&rt, &mut utc, "Date.parse('Jan 1 2000 GMT')"),
        ValueView::Number(946684800000.0)
    );
    assert_eq!(
        evaluate(&rt, &mut host, "Date.parse('Jan 1 2000')"),
        ValueView::Number(946681200000.0)
    );
    assert_eq!(host.clock_reads(), 0);
}
