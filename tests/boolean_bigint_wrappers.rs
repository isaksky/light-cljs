//! Synthetic JavaScript fixtures, not light-cljs output.

use big_boy_pants_js::frontend::{ScriptOptions, compile_script, compile_script_with};
use big_boy_pants_js::{Budget, Host, RunOutcome, Runtime, ValueView};

struct NoHost;
impl Host for NoHost {
    fn effect(
        &mut self,
        _: big_boy_pants_js::HostCall,
    ) -> Result<big_boy_pants_js::Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn check(source: &str) {
    let code = compile_script("wrappers.js", source).unwrap();
    let runtime = Runtime::new();
    let task = runtime.spawn(&code, &[]).unwrap();
    for _ in 0..100_000 {
        match task.run_with(Budget::instructions(7), &mut NoHost).unwrap() {
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                return;
            }
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            other => panic!("{source}: {other:?}"),
        }
    }
    panic!("wrapper fixture exceeded instruction bound: {source}");
}

#[test]
fn boolean_constructor_and_boxing() {
    for source in [
        "typeof Boolean === 'function' && globalThis.Boolean === Boolean && Boolean.length === 1 && Boolean.name === 'Boolean'",
        "Boolean() === false && Boolean(undefined) === false && Boolean(null) === false && Boolean(0) === false && Boolean(-0) === false && Boolean(NaN) === false && Boolean('') === false && Boolean(0n) === false",
        "Boolean(1) && Boolean('false') && Boolean(1n) && Boolean(Symbol()) && Boolean({}) && Boolean(new Boolean(false))",
        "let n = 0; let x = {valueOf() { n++; throw 1; }, toString() { n++; throw 2; }}; Boolean(x) && new Boolean(x).valueOf() === true && n === 0",
        "let n = 0; let b = new Boolean(false, n++); n === 1 && typeof b === 'object' && b.valueOf() === false && b.toString() === 'false' && b instanceof Boolean && b instanceof Object",
        "let a = Object(true), b = new Object(false); a.valueOf() === true && b.valueOf() === false && Object(a) === a && Object.getPrototypeOf(a) === Boolean.prototype && Object.keys(a).length === 0",
        "Boolean.prototype.valueOf() === false && Boolean.prototype.toString() === 'false' && Object.getPrototypeOf(Boolean.prototype) === Object.prototype && Boolean.prototype.constructor === Boolean",
        "true.valueOf() === true && false.toString() === 'false' && true.constructor === Boolean && Object.getPrototypeOf(true) === Boolean.prototype && true.__proto__ === Boolean.prototype",
        "let b = new Boolean; b.x = 7; b.x === 7 && Boolean.prototype.x === undefined && Object.getOwnPropertyNames(b).join() === 'x'",
        "Object.prototype.toString.call(true) === '[object Boolean]' && Object.prototype.toString.call(Object(false)) === '[object Boolean]'",
        "let B = Boolean.bind(null, false); new B().valueOf() === false && B() === false && Boolean.call(null, 1) === true && Boolean.apply(null, [0]) === false",
        "let d = Object.getOwnPropertyDescriptor(Boolean, 'prototype'); d.value === Boolean.prototype && !d.writable && !d.enumerable && !d.configurable",
        "let ok = true; for (let k of ['toString', 'valueOf']) { let d = Object.getOwnPropertyDescriptor(Boolean.prototype,k); ok = ok && d.writable && !d.enumerable && d.configurable && d.value.length === 0 && d.value.name === k; } ok",
        "let n = 0; for (let x of [undefined,null,0,'false',1n,{},Object.create(Boolean.prototype),Object(1n)]) { for (let k of ['valueOf','toString']) { try { Boolean.prototype[k].call(x); } catch(e) { if (e instanceof TypeError) n++; } } } n === 16",
        "Object.prototype.valueOf.call(false).valueOf() === false && Array.prototype.join.call(true) === '' && Array.from(false).length === 0",
    ] {
        check(source);
    }
}

#[test]
fn bigint_wrappers_and_brand_checks() {
    for source in [
        "let b = Object(123n); typeof b === 'object' && b.valueOf() === 123n && b.toString() === '123' && Object(b) === b && Object.getPrototypeOf(b) === BigInt.prototype && b instanceof BigInt",
        "let b = new Object(-255n); b.toString(16) === '-ff' && b.toString(2) === '-11111111' && b.constructor === BigInt && Object.keys(b).length === 0",
        "let b = Object(1n); b.x = 7; b.x === 7 && BigInt.prototype.x === undefined && Object.prototype.valueOf.call(1n).valueOf() === 1n && Array.prototype.join.call(1n) === ''",
        "let n = 0; for (let x of [undefined,null,1,true,'1',{},BigInt.prototype,Object.create(BigInt.prototype),Object(true)]) { for (let k of ['valueOf','toString']) { try { BigInt.prototype[k].call(x); } catch(e) { if (e instanceof TypeError) n++; } } } n === 18",
        "let n = 0; try { BigInt.prototype.toString.call({}, {valueOf() { n++; return 10; }}); } catch(e) { n += e instanceof TypeError ? 10 : 100; } n === 10",
        "let n = 0; let b = Object(255n); b.toString({valueOf() { n++; return 16; }}) === 'ff' && n === 1",
        "let b = Object(255n); BigInt.prototype.toString.call(b,16) === 'ff' && BigInt.prototype.valueOf.apply(b,[]) === 255n && BigInt.asIntN(8,b) === -1n",
        "let n = 0; try { new BigInt(1); } catch(e) { n = e instanceof TypeError; } n",
        "let b = Object(3n); BigInt(b) === 3n && Number(b) === 3 && String(b) === '3' && b + 2n === 5n && b == 3n && b !== 3n",
        "let b = Object(true); String(b) === 'true' && Number(b) === 1 && b + 2 === 3 && b == true && b !== true",
        "let b = Object(false); b.valueOf = () => 7; Number(b) === 7 && String(b) === 'false'",
    ] {
        check(source);
    }
}

#[test]
fn wrapper_json_uses_internal_boolean_and_bigint_data() {
    for source in [
        "JSON.stringify([Object(true),new Boolean(false)]) === '[true,false]' && JSON.stringify({x:Object(false)}) === '{\"x\":false}' && JSON.stringify(Boolean.prototype) === 'false'",
        "let b = Object(false); b.valueOf = () => {throw 1;}; b.toString = () => {throw 2;}; b.x = 3; JSON.stringify(b) === 'false'",
        "let n = 0; for (let b of [Object(1n), {x:Object(1n)}, [Object(1n)]]) { try { JSON.stringify(b); } catch(e) { if(e instanceof TypeError) n++; } } n === 3",
        "let b = Object(1n); b.valueOf = () => 1; b.toString = () => '1'; let ok; try { JSON.stringify(b); } catch(e) { ok = e instanceof TypeError; } ok",
        "let b = Object(1n); b.toJSON = function(k) { return this.valueOf().toString() + k; }; JSON.stringify({x:b}) === '{\"x\":\"1x\"}'",
        "let b = Object(false); b.toJSON = function() { return 7; }; JSON.stringify(b) === '7'",
        "JSON.stringify({x:1},null,Object(true)) === '{\"x\":1}' && JSON.stringify({x:1},null,Object(1n)) === '{\"x\":1}'",
    ] {
        check(source);
    }
}

#[test]
fn boolean_and_bigint_with_environments() {
    for source in [
        "let ok; with (true) { ok = valueOf() === true && toString() === 'true' && constructor === Boolean; } ok",
        "let ok; with (1n) { ok = valueOf() === 1n && toString() === '1' && constructor === BigInt; } ok",
    ] {
        let code = compile_script_with("with-wrappers.js", source, ScriptOptions::SLOPPY).unwrap();
        let runtime = Runtime::new();
        let result = runtime.spawn(&code, &[]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{source}"
        );
    }
}

#[test]
fn boolean_and_bigint_generic_array_receivers() {
    for (primitive, constructor) in [("false", "Boolean"), ("1n", "BigInt")] {
        for method in ["sort", "reverse", "fill"] {
            check(&format!(
                "let b = Array.prototype.{method}.call({primitive}); typeof b === 'object' && b.valueOf() === {primitive} && Object.getPrototypeOf(b) === {constructor}.prototype"
            ));
        }
        for source in [
            "Array.prototype.unshift.call(PRIMITIVE,7) === 1 && Array.prototype.splice.call(PRIMITIVE,0,0,7).length === 0",
            "Array.prototype.indexOf.call(PRIMITIVE,7) === -1 && !Array.prototype.includes.call(PRIMITIVE,undefined)",
            "let n = 0; let a = Array.prototype.map.call(PRIMITIVE,() => { n++; }); Array.isArray(a) && a.length === 0 && n === 0 && Array.prototype.reduce.call(PRIMITIVE,() => { n++; },7) === 7 && n === 0",
            "let a = Array.prototype.slice.call(PRIMITIVE), b = Array.prototype.toReversed.call(PRIMITIVE), c = Array.prototype.flat.call(PRIMITIVE); Array.isArray(a) && Array.isArray(b) && Array.isArray(c) && a.length === 0 && b.length === 0 && c.length === 0",
            "let a = Array.prototype.concat.call(PRIMITIVE,7); a.length === 2 && typeof a[0] === 'object' && a[0].valueOf() === PRIMITIVE && a[1] === 7",
        ] {
            check(&source.replace("PRIMITIVE", primitive));
        }
    }
}

#[test]
fn wrappers_are_task_owned_and_survive_migration() {
    let runtime = Runtime::new();
    for source in ["Object(true)", "Object(123456789012345678901234567890n)"] {
        let code = compile_script("wrapper-owner.js", source).unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(value) = task.run().unwrap() else {
            panic!()
        };
        assert!(matches!(
            runtime.publish(&value),
            Err(big_boy_pants_js::PublishError::NotShareable { .. })
        ));
    }
    let code = compile_script("wrapper-migration.js", "let a = Object(false), b = Object(123456789012345678901234567890n); let calls = 0; let radix = {valueOf() { calls++; __rt.need(302, 1); return 16; }}; a.valueOf() === false && b.toString(radix) === '18ee90ff6c373e0ee4e3f0ad2' && calls === 1").unwrap();
    let mut task = runtime.spawn(&code, &[]).unwrap();
    let mut waits = 0;
    for _ in 0..10_000 {
        let (next, result) = std::thread::spawn(move || {
            let result = task.run_with(Budget::instructions(5), &mut NoHost).unwrap();
            (task, result)
        })
        .join()
        .unwrap();
        task = next;
        match result {
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            RunOutcome::Complete(v) => {
                assert_eq!(v.view(), ValueView::Bool(true));
                assert_eq!(waits, 1);
                return;
            }
            RunOutcome::Waiting(_) => {
                waits += 1;
                assert_eq!(waits, 1);
                runtime.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = runtime.root(big_boy_pants_js::Primitive::Null);
                task = runtime
                    .complete(
                        request.key(),
                        request.producer(),
                        big_boy_pants_js::Completion::Ready(&value),
                    )
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("migration exceeded bound");
}

#[test]
fn wrappers_use_small_worker_stacks() {
    for source in [
        "new Boolean(false).valueOf() === false",
        "Object(255n).valueOf() === 255n",
        "Boolean(Object(false)) === true",
        "Object(255n).toString(16) === 'ff'",
        "JSON.stringify(Object(false)) === 'false'",
    ] {
        let runtime = Runtime::new();
        let init = compile_script("init", "Object").unwrap();
        runtime.spawn(&init, &[]).unwrap().run().unwrap();
        let code = compile_script("small-wrapper-worker", source).unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let result = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let RunOutcome::Complete(value) = task.run().unwrap() else {
                    panic!("wrapper execution failed")
                };
                value.view()
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(result, ValueView::Bool(true));
    }
}

#[test]
fn boolean_primitive_reads_allocate_no_wrappers() {
    let runtime = Runtime::new();
    let init = compile_script("init", "Boolean").unwrap();
    runtime.spawn(&init, &[]).unwrap().run().unwrap();
    let code = big_boy_pants_js::frontend::compile_function("function f() { let b = false; for(let i = 0; i < 100; i++) { if(b.valueOf() !== false || b.toString() !== 'false') return false; } return true; }").unwrap();
    let before = runtime.heap_stats().objects;
    let result = runtime.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
    assert_eq!(runtime.heap_stats().objects, before);
}
