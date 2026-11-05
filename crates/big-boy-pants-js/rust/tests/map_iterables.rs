//! Synthetic fixtures, not light-cljs output.
use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::{Budget, RunOutcome, Runtime, ValueView};

struct NoEffects;
impl big_boy_pants_js::host::Host for NoEffects {
    fn effect(
        &mut self,
        _: big_boy_pants_js::host::HostCall,
    ) -> Result<big_boy_pants_js::Primitive, String> {
        panic!("unexpected host effect")
    }
}

fn check(source: &str) {
    let rt = Runtime::new();
    let code = compile_script("map-iterables.js", source).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("fixture failed: {source}");
    };
    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
}

#[test]
fn map_and_weak_map_accept_entry_iterables() {
    for source in [
        "new Map([[1,2]]).get(1) === 2",
        "const k={}; new WeakMap([[k,2]]).get(k) === 2",
        "new Map().size === 0 && new Map(null).size === 0 && new Map(undefined).size === 0 && !new WeakMap(null).has({})",
        "let m = new Map([[1,2],[1,3],[-0,4],[NaN,5],[1n,6],[1n,7]]); m.size === 4 && m.get(1) === 3 && m.get(0) === 4 && m.get(NaN) === 5 && m.get(1n) === 7",
        "function* entries() { yield {0: 1, 1: 2}; yield [3]; yield {}; } let m = new Map(entries()); m.get(1) === 2 && m.has(3) && m.get(3) === undefined && m.has(undefined)",
        "let k = {}, s = Symbol(); let m = new WeakMap([[k,1],[s,2],[k,3]]); m.get(k) === 3 && m.get(s) === 2",
        "let B = Map.bind(null, [[1,2]]); new B().get(1) === 2",
        "new Map([]).size === 0 && new Map('').size === 0 && new WeakMap([]) instanceof WeakMap",
        "new Map([Object.create({0:1,1:2})]).get(1) === 2",
        "let it = {[Symbol.iterator]() {return this;}, next() {return {done:true, get value() {throw 98;}};}, get return() {throw 99;}}; new Map(it).size === 0",
    ] {
        check(source);
    }
}

#[test]
fn entry_failures_close_once_and_preserve_original_throw() {
    for constructor in ["Map", "WeakMap"] {
        for entry in [
            "null",
            "undefined",
            "1",
            "'ab'",
            "true",
            "1n",
            "Symbol()",
            "{get 0() { throw 17; }, get 1() { throw 99; }}",
            "{0: {}, get 1() { throw 17; }}",
        ] {
            for close in ["return {};", "throw 99;", "return 1;"] {
                check(&format!(
                    r#"
                    let n = 0, closed = 0, ok = false;
                    let it = {{[Symbol.iterator]() {{ return this; }}, next() {{ n++; return {{value: {entry}}}; }},
                        return() {{ if (this !== it || arguments.length !== 0) throw 98; closed++; {close} }} }};
                    try {{ new {constructor}(it); }} catch(e) {{ ok = {expected}; }}
                    ok && n === 1 && closed === 1
                "#,
                    expected = if entry.contains("throw 17") {
                        "e === 17"
                    } else {
                        "e instanceof TypeError"
                    }
                ));
            }
        }
    }
    check(
        "let closed = 0, ok = false; function* g() { try { yield [1,2]; } finally { closed++; } } try { new WeakMap(g()); } catch(e) { ok = e instanceof TypeError; } ok && closed === 1",
    );
}

#[test]
fn iterator_protocol_failures_do_not_close() {
    for constructor in ["Map", "WeakMap"] {
        for next in [
            "next() { throw 17; }",
            "next() { return 1; }",
            "next() { return {get done() { throw 17; }}; }",
            "next() { return {get value() { throw 17; }}; }",
            "next: 0",
            "get next() { throw 17; }",
        ] {
            check(&format!(
                r#"
                let closed = 0, ok = false;
                let it = {{[Symbol.iterator]() {{ return this; }}, {next}, return() {{ closed++; return {{}}; }} }};
                try {{ new {constructor}(it); }} catch(e) {{ ok = {expected}; }}
                ok && closed === 0
            "#,
                expected = if next.contains("throw 17") {
                    "e === 17"
                } else {
                    "e instanceof TypeError"
                }
            ));
        }
        for value in ["true", "1", "({})", "'ab'"] {
            check(&format!(
                "let ok = false; try {{ new {constructor}({value}); }} catch(e) {{ ok = e instanceof TypeError; }} ok"
            ));
        }
    }
}

#[test]
fn map_iterable_construction_on_small_worker_stack() {
    let rt = Runtime::new();
    rt.spawn(&compile_script("init", "Map").unwrap(), &[])
        .unwrap()
        .run()
        .unwrap();
    let task = rt.spawn(&compile_script("map-stack", "let k = {}; let m = new Map([[k,7]]), w = new WeakMap([[k,8]]); m.get(k) + w.get(k) === 15").unwrap(), &[]).unwrap();
    let outcome = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || task.run().unwrap())
        .unwrap()
        .join()
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}

#[test]
fn map_iterable_frames_survive_collection_and_migration() {
    for constructor in ["Map", "WeakMap"] {
        let source = format!(
            r#"
            let C = {constructor}, calls = 0;
            let keys = [{{}}, {{}}];
            function* g() {{ for (let i = 0; i < keys.length; i++) yield {{0: keys[i], get 1() {{ calls++; return {{n: i + 7}}; }} }}; }}
            let m = new C(g());
            m.get(keys[0]).n === 7 && m.get(keys[1]).n === 8 && calls === 2
        "#
        );
        let rt = Runtime::new();
        let task = rt
            .spawn(&compile_script("map-gc.js", &source).unwrap(), &[])
            .unwrap();
        let mut yields = 0;
        let mut complete = false;
        for _ in 0..4000 {
            let outcome = std::thread::scope(|scope| {
                scope
                    .spawn(|| task.run_with(Budget::instructions(1), &mut NoEffects))
                    .join()
                    .unwrap()
                    .unwrap()
            });
            match outcome {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(v) => {
                    assert_eq!(v.view(), ValueView::Bool(true));
                    complete = true;
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(complete);
        assert!(yields > 100);
    }
}

#[test]
fn map_entries_are_iterable() {
    for source in [
        "[...new Map([[1, 2]])][0][1] === 2",
        "let sum = 0; for (const [k, v] of new Map([[1, 2], [3, 4]])) sum += k + v; sum === 10",
        "const [[k, v], ...rest] = new Map([[1, 2], [3, 4]]); k === 1 && v === 2 && rest[0][1] === 4",
        "let m = new Map([[1, 2]]), it = m.entries(), first = it.next(), last = it.next(); first.done === false && first.value[0] === 1 && first.value[1] === 2 && last.done && last.value === undefined && it[Symbol.iterator]() === it",
        "let m = new Map([[1, 2], [3, 4]]); [...m.keys()].join() === '1,3' && [...m.values()].join() === '2,4' && m[Symbol.iterator] === m.entries && m.entries.name === 'entries' && m.entries.length === 0 && Object.prototype.toString.call(m.entries()) === '[object Map Iterator]'",
        "let m = new Map([[1, 2], [3, 4]]), a = m.entries(), b = m.keys(); a.next(); m.delete(3); m.set(5, 6); let x = a.next().value; m.clear(); m.set(7, 8); let y = a.next().value; a.next(); m.set(9, 10); x[0] === 5 && x[1] === 6 && y[0] === 7 && a.next().done && b.next().value === 7",
        "let m = new Map([[1, 2]]), i = m.values(); m.set(1, 9); i.next().value === 9",
        "let n = new Map().entries().next, m = new Map([[3, 4]]); n.call(m.keys()).value === 3 && n.call(m.values()).value === 4",
        "let n = 0; for (let value of [{}, [], new Set().values(), [1].values(), 'a'[Symbol.iterator]()]) { try { new Map().entries().next.call(value); } catch(e) { if(e instanceof TypeError)n++; } } n === 5",
        "let n = 0; for (let f of [Map.prototype.keys, Map.prototype.values, Map.prototype.entries]) { try { f.call(new Set()); } catch(e) { if(e instanceof TypeError)n++; } } n === 3",
    ] {
        check(source);
    }
}

#[test]
fn map_iteration_observes_overrides_and_abrupt_completion() {
    for source in [
        "let m = new Map([[1, 2]]), reads = 0; Object.defineProperty(m, Symbol.iterator, {get() { reads++; return function*() { yield [7, 8]; }; }}); [...m][0][1] === 8 && reads === 1",
        "let m = new Map([[1, 2]]); m.entries = function() { throw 99; }; [...m][0][1] === 2",
        "let m = new Map([[1, 2]]), calls = 0, i = m.entries(), next = i.next; i.next = function() { calls++; return next.call(this); }; m[Symbol.iterator] = () => i; [...m][0][1] === 2 && calls === 2",
        "let m = new Map([[1, 2]]), closed = 0, i = m.entries(); i.return = function() { closed++; return {}; }; m[Symbol.iterator] = () => i; for (let entry of m) break; closed === 1",
        "let m = new Map([[1, 2]]), closed = 0, ok = false, i = m.entries(); i.return = function() { closed++; throw 99; }; m[Symbol.iterator] = () => i; try { for (let [k,v] of m) throw 17; } catch(e) { ok = e === 17; } ok && closed === 1",
        "let m = new Map([[1, 2]]), closed = 0, i = m.entries(); i.return = function() { closed++; return {}; }; m[Symbol.iterator] = () => i; let [entry] = m; entry[1] === 2 && closed === 1",
        "let m = new Map(), ok = false; m[Symbol.iterator] = function() { throw 17; }; try { [...m]; } catch(e) { ok = e === 17; } ok",
    ] {
        check(source);
    }
}

#[test]
fn map_cursors_survive_collection_and_migration() {
    let rt = Runtime::new();
    let code = compile_script("map-cursor-gc", "let m = new Map([[{n:1}, {n:2}], [{n:3}, {n:4}]]), sum = 0; for (let [k,v] of m) sum += k.n + v.n; sum === 10").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    for _ in 0..2000 {
        let outcome = std::thread::scope(|scope| {
            scope
                .spawn(|| task.run_with(Budget::instructions(1), &mut NoEffects))
                .join()
                .unwrap()
                .unwrap()
        });
        match outcome {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(v) => {
                assert_eq!(v.view(), ValueView::Bool(true));
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("map cursor did not finish");
}
