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
    let task = rt
        .spawn(&compile_script("sets.js", source).unwrap(), &[])
        .unwrap();
    match task.run().unwrap() {
        RunOutcome::Complete(v) => assert_eq!(v.view(), ValueView::Bool(true), "{source}"),
        other => panic!("{other:?}: {source}"),
    }
}
#[test]
fn constructors_and_membership() {
    for source in [
        "typeof Set === 'function' && typeof WeakSet === 'function' && new Set([1,2,2]).size === 2",
        "let s = new Set([NaN,NaN,-0,0,1n,1n,1]); s.size === 4 && s.has(NaN) && s.has(0) && s.has(1n) && s.add(7) === s && s.delete(1) && !s.delete(1)",
        "let k = {}, y = Symbol(), s = new WeakSet([k,y,k]); s.has(k) && s.has(y) && s.add(k) === s && s.delete(k) && !s.has(k) && !s.delete(k) && !s.has(1) && !s.delete(Symbol.for('x'))",
        "let s = new Set('a😀a'); s.size === 2 && s.has('😀') && new Set(null).size === 0 && new Set(undefined).size === 0 && new WeakSet(null) instanceof WeakSet",
        "function* g() { yield 2; yield 3; yield 2; } let s = new Set(g()); s.size === 2 && s.has(3)",
        "let C = Set.bind(null, [1,2]); new C().size === 2",
        "let s = new Set([1]); s.clear() === undefined && s.size === 0 && !s.has(1)",
        "Set.length === 0 && WeakSet.length === 0 && Set.prototype.add.length === 1 && Set.prototype.forEach.length === 1 && Set.prototype.values.length === 0",
        "let s = new WeakSet(); s.size === undefined && s.clear === undefined && s.forEach === undefined && s[Symbol.iterator] === undefined",
        "let s = new Set(); s.x = 4; Object.keys(s).join() === 'x' && Object.getOwnPropertyDescriptor(s, 'x').value === 4 && delete s.x && Object.getPrototypeOf(s) === Set.prototype && s instanceof Set",
    ] {
        check(source);
    }
}
#[test]
fn set_cursors_and_callbacks_observe_mutations() {
    for source in [
        "Set.prototype.values === Set.prototype.keys && Set.prototype.values === Set.prototype[Symbol.iterator]",
        "let s = new Set([-0,2]), it = s.values(); let a = it.next(); !a.done && Object.is(a.value, 0) && it[Symbol.iterator]() === it && it.next().value === 2 && it.next().done && it.next().done",
        "let s = new Set([1,2]), it = s.entries(), a = it.next().value; a[0] === 1 && a[1] === 1 && it.next().value[1] === 2 && it.next().done",
        "let s = new Set([1,2,3]), a = s.values(), b = s.values(); a.next(); s.delete(2); s.add(4); b.next(); a.next().value === 3 && b.next().value === 3 && a.next().value === 4 && b.next().value === 4",
        "let s = new Set([1,2]), it = s.values(); it.next(); s.clear(); s.add(3); it.next().value === 3 && it.next().done && (s.add(4), it.next().done)",
        "let s = new Set([1,2]), log = '', that = {}; let r = s.forEach(function(v,k,o) { if (v !== k || o !== s || this !== that || arguments.length !== 3) throw 99; log += v; if (v === 1) { s.delete(2); s.add(3); } }, that); r === undefined && log === '13'",
        "let s = new Set([1,2]), log = ''; s.forEach(v => { log += v; if (v === 1) { s.clear(); s.add(3); } }); log === '13'",
        "let log = ''; for (let v of new Set([3,1,3,2])) log += v; log === '312' && Array.from(new Set([4,5])).join() === '4,5'",
        "let s = new Set([1]); Object.freeze(s); s.add(2); s.delete(1); Object.isFrozen(s) && !Object.isExtensible(s) && s.size === 1 && s.has(2)",
        "let s = new WeakSet(), k = {}; Object.preventExtensions(s); s.add(k); s.has(k) && s.delete(k)",
    ] {
        check(source);
    }
}
#[test]
fn brands_and_invalid_inputs() {
    for source in [
        "let n = 0; for (let C of [Set, WeakSet]) { try { C(); } catch(e) { if (e instanceof TypeError) n++; } } n === 2",
        "let n = 0; for (let v of [1, true, {}, 1n]) { try { new Set(v); } catch(e) { if (e instanceof TypeError) n++; } } n === 4",
        "let n = 0; for (let v of [1, null, undefined, 'a', Symbol.for('x')]) { try { new WeakSet().add(v); } catch(e) { if (e instanceof TypeError) n++; } } n === 5",
        "let n = 0; for (let f of [Set.prototype.add,Set.prototype.has,Set.prototype.delete,Set.prototype.clear,Set.prototype.values,Set.prototype.entries,Set.prototype.forEach,WeakSet.prototype.add,WeakSet.prototype.has,WeakSet.prototype.delete]) { try { f.call(new Map(), () => 0); } catch(e) { if (e instanceof TypeError) n++; } } n === 10",
        "let n = 0; try { Map.prototype.has.call(new Set(), 1); } catch(e) { if (e instanceof TypeError) n++; } try { WeakMap.prototype.has.call(new WeakSet(), {}); } catch(e) { if (e instanceof TypeError) n++; } try { new Set().forEach(0); } catch(e) { if (e instanceof TypeError) n++; } n === 3",
    ] {
        check(source);
    }
}
#[test]
fn iterable_errors_and_close() {
    for next in [
        "next() { throw 17; }",
        "next() { return {get done() { throw 17; }}; }",
        "next() { return {get value() { throw 17; }}; }",
    ] {
        for ctor in ["Set", "WeakSet"] {
            check(&format!(
                "let closed = 0, ok = false; let it = {{[Symbol.iterator]() {{return this;}}, {next}, return() {{closed++; return {{}};}}}}; try {{new {ctor}(it);}} catch(e) {{ok = e === 17;}} ok && closed === 0"
            ));
        }
    }
    for close in ["return {};", "throw 99;", "return 1;"] {
        check(&format!(
            "let closed = 0, ok = false; let it = {{[Symbol.iterator]() {{return this;}}, next() {{return {{value: 1}};}}, return() {{closed++; {close}}}}}; try {{new WeakSet(it);}} catch(e) {{ok = e instanceof TypeError;}} ok && closed === 1"
        ));
    }
}
#[test]
fn set_frames_survive_collection_and_migration() {
    let rt = Runtime::new();
    let source = "let a = {}, b = Symbol(), w = new WeakSet([a,b]); function* g() {yield a; yield b; yield a;} let s = new Set(g()), n = 0; s.forEach((v,k,o) => { if (v === k && o === s && w.has(v)) n++; }); let it = s.entries(); it.next().value[0] === a && it.next().value[1] === b && it.next().done && n === 2";
    let task = rt
        .spawn(&compile_script("sets-gc.js", source).unwrap(), &[])
        .unwrap();
    let mut yields = 0;
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
                assert!(yields > 100);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("instruction bound exceeded");
}

#[test]
fn set_iterable_construction_on_small_worker_stack() {
    let rt = Runtime::new();
    rt.spawn(&compile_script("init", "Set").unwrap(), &[])
        .unwrap()
        .run()
        .unwrap();
    let task = rt
        .spawn(
            &compile_script(
                "set-stack",
                "let k = {}; new Set([k,k]).size === 1 && new WeakSet([k]).has(k)",
            )
            .unwrap(),
            &[],
        )
        .unwrap();
    let outcome = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || task.run().unwrap())
        .unwrap()
        .join()
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}

#[test]
fn set_iterator_next_enforces_brands() {
    check(
        r#"
        let sn = Object.getPrototypeOf(new Set().values()).next;
        let an = Object.getPrototypeOf([].values()).next;
        let n = 0;
        for (let exhausted of [false,true]) {
            let s = new Set([7]).values(), a = [9].values();
            if (exhausted) {s.next(); s.next(); a.next(); a.next();}
            for (let pair of [[an,s],[sn,a],[sn,{}],[sn,null]]) {
                try {pair[0].call(pair[1]);} catch(e) {if (e instanceof TypeError) n++;}
            }
            if (!exhausted && (s.next().value !== 7 || a.next().value !== 9)) throw 99;
        }
        let s = new Set([3]), values = s.values(), entries = s.entries();
        let v = sn.call(values), e = sn.call(entries);
        n === 8 && v.value === 3 && !v.done && !e.done && e.value[0] === 3 && e.value[1] === 3 && sn.call(values).done && sn.call(entries).done
    "#,
    );
}

#[test]
fn sets_preserve_ordinary_object_protocols() {
    for kind in ["Set", "WeakSet"] {
        check(&format!(
            r#"
            let s = new {kind}([{{hidden:17}}]);
            s.a = 1; s.b = {{n:2}}; s[Symbol()] = 99;
            Object.defineProperty(s,'secret',{{value:99}});
            let ok = JSON.stringify(s) === '{{"a":1,"b":{{"n":2}}}}';
            let p = {{tag:7}};
            Object.setPrototypeOf(s,p);
            ok = ok && Object.getPrototypeOf(s) === p && s.tag === 7;
            let k = {{}}; {kind}.prototype.add.call(s,k);
            class Base {{constructor(o) {{return o;}}}}
            class Stamp extends Base {{#data = {{n:23}}; static read(o) {{return o.#data.n;}}}}
            let stamped = new Stamp(s);
            ok && stamped === s && Stamp.read(s) === 23 && {kind}.prototype.has.call(s,k) && {kind}.prototype.delete.call(s,k)
        "#
        ));
    }
}
