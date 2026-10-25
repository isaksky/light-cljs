//! Synthetic JavaScript fixtures, not light-cljs output.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, Host, RunOutcome, Runtime, ValueView};

struct NoHost;
impl Host for NoHost {
    fn effect(&mut self, _: clj_quickjs::HostCall) -> Result<clj_quickjs::Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn check(source: &str) {
    let code = compile_script("builtin-subclasses.js", source).unwrap();
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
    panic!("fixture exceeded instruction bound: {source}");
}

#[test]
fn array_subclass_preserves_exotic_length_and_forwarding() {
    check(
        "class A extends Array {} let a = new A(1,2,3); a.length === 3 && a[2] === 3 && a instanceof A && a instanceof Array && Array.isArray(a) && Object.getPrototypeOf(a) === A.prototype",
    );
    check(
        "class A extends Array { constructor(n) { super(n); this.tag = 7; } } class B extends A {} let b = new B(4); b.length === 4 && !(0 in b) && b.tag === 7 && b instanceof B && b instanceof A && b.push(8) === 5 && b[4] === 8",
    );
    check(
        "class A extends Array {} let ok; try { new A(-1); } catch(e) { ok = e instanceof RangeError; } ok",
    );
}

#[test]
fn error_subclasses_preserve_brand_stack_name_and_cause() {
    check(
        "class E extends Error {} E.prototype.name = 'E'; let cause = {}; let e = new E('x',{cause}); e instanceof E && e instanceof Error && e.message === 'x' && e.cause === cause && e.toString() === 'E: x' && typeof e.stack === 'string' && e.stack.indexOf('builtin-subclasses.js:') >= 0",
    );
    check(
        "let ok = true; for (let C of [TypeError,RangeError,ReferenceError,SyntaxError,EvalError,URIError,InternalError]) { class E extends C {} let e = new E('x'); ok = ok && e instanceof E && e instanceof C && e instanceof Error && e.message === 'x' && e.name === C.name; } ok",
    );
    check(
        "class E extends SuppressedError {} let e = new E(1,2,'x'); e instanceof E && e.error === 1 && e.suppressed === 2 && e.message === 'x'",
    );
}

#[test]
fn collection_subclasses_use_derived_set_and_add() {
    check(
        "let calls = 0; class M extends Map { set(k,v) { calls++; return super.set(k,v+1); } } let m = new M([[1,2],[3,4]]); m instanceof M && m instanceof Map && m.get(1) === 3 && calls === 2 && m.size === 2",
    );
    check(
        "let calls = 0; class W extends WeakMap { set(k,v) { calls++; return super.set(k,v+1); } } let k = {}; let w = new W([[k,2]]); w instanceof W && w.get(k) === 3 && calls === 1",
    );
    check(
        "let calls = 0; class S extends Set { add(k) { calls++; return super.add(k+1); } } let s = new S([1,2]); s instanceof S && s instanceof Set && s.has(2) && s.has(3) && calls === 2",
    );
    check(
        "let calls = 0; class W extends WeakSet { add(k) { calls++; return super.add(k); } } let k = {}; let w = new W([k]); w instanceof W && w.has(k) && calls === 1",
    );
}

#[test]
fn date_regexp_and_wrapper_subclasses_keep_internal_slots() {
    check(
        "class D extends Date {} let d = new D(123); d instanceof D && d instanceof Date && d.getTime() === 123",
    );
    check(
        "class R extends RegExp {} let r = new R('a','g'); r instanceof R && r instanceof RegExp && r.test('a') && r.lastIndex === 1 && r.source === 'a'",
    );
    check(
        "class B extends Boolean {} class N extends Number {} class S extends String {} let b = new B(0), n = new N({valueOf(){return 12;}}), s = new S({toString(){return 'abc';}}); b instanceof B && b.valueOf() === false && n instanceof N && n.valueOf() === 12 && s instanceof S && s.valueOf() === 'abc' && s.length === 3 && s[1] === 'b'",
    );
    check(
        "let n = 0; for (let C of [Symbol,BigInt]) { try { class X extends C {} new X(1); } catch(e) { if(e instanceof TypeError) n++; } } n === 2",
    );
}

#[test]
fn promise_subclass_keeps_brand_and_runs_executor() {
    check(
        "let n = 0; class P extends Promise {} let p = new P(resolve => { n++; resolve(7); }); p instanceof P && p instanceof Promise && Object.getPrototypeOf(p) === P.prototype && n === 1",
    );
    check(
        "class P extends Promise {} let p = new P(() => {throw 1;}); p instanceof P && typeof p.then === 'function'",
    );
}

#[test]
fn buffer_typed_array_and_function_subclasses() {
    check(
        "class B extends ArrayBuffer {} let b = new B(8); class U extends Uint8Array {} let u = new U(b); u[0] = 7; b instanceof B && b.byteLength === 8 && u instanceof U && u instanceof Uint8Array && u.length === 8 && u[0] === 7",
    );
    check(
        "class F extends Function {} let f = new F('x','return x + 1'); f instanceof F && f instanceof Function && f(2) === 3",
    );
}

#[test]
fn disposable_and_proxy_constructor_exceptions() {
    check(
        "class S extends DisposableStack {} class A extends AsyncDisposableStack {} let s = new S, a = new A; s instanceof S && a instanceof A && s.disposed === false && a.disposed === false",
    );
    // Proxy has no prototype property, so it cannot be a class heritage value.
    check(
        "let ok; try { class P extends Proxy {} } catch(e) { ok = e instanceof TypeError; } ok && Proxy.prototype === undefined",
    );
}

#[test]
fn transparent_proxy_new_target_preserves_subclass_identity() {
    check(
        "let C = new Proxy(Object,{}); let o = new C(7); Object.getPrototypeOf(o) === Object.prototype && Object.keys(o).length === 0",
    );
    check(
        "class A extends Array {} let C = new Proxy(A,{}); let a = new C(1,2); a instanceof A && a.length === 2 && Object.getPrototypeOf(a) === A.prototype",
    );
}

#[test]
fn bound_builtin_super_and_argument_forwarding() {
    check(
        "class A extends Array {} let B = A.bind(null,1); let a = new B(2); a instanceof A && a.length === 2 && a[0] === 1 && a[1] === 2",
    );
    check(
        "let n = 0; let B = Array.bind(null,1); B.prototype = Array.prototype; class A extends B {} let a = new A(2); a instanceof A && a.length === 2 && a[0] === 1 && a[1] === 2",
    );
}

#[test]
fn promise_subclass_and_resolver_cannot_publish_mutable_prototype() {
    for (source, consumer) in [
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return Object.getPrototypeOf(p);}",
        ),
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return Promise.prototype.then.call(p,()=>{});}",
        ),
        (
            "let resolve; class P extends Promise {} new P(r => {resolve = r;}); resolve;",
            "function consume(r){return r(1);}",
        ),
    ] {
        let runtime = Runtime::new();
        let code = compile_script("local-promise.js", source).unwrap();
        let RunOutcome::Complete(value) = runtime.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("expected constructor result");
        };
        assert!(runtime.publish(&value).is_err(), "{source}");
        let consumer = clj_quickjs::frontend::compile_function(consumer).unwrap();
        let result = runtime.spawn(&consumer, &[&value]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Failed(e) if matches!(e.kind(), clj_quickjs::VmErrorKind::ForeignObject { .. })),
            "{source}"
        );
    }
}

#[test]
fn other_slot_bearing_constructors_use_subclass_prototypes() {
    check(
        "class W extends WeakRef {} let target = {}; let w = new W(target); w instanceof W && w.deref() === target",
    );
    check(
        "class F extends FinalizationRegistry {} let f = new F(() => {}); f instanceof F && f instanceof FinalizationRegistry",
    );
    check(
        "let ok = true; for (let C of [Int8Array,Uint8Array,Uint8ClampedArray,Int16Array,Uint16Array,Int32Array,Uint32Array,Float16Array,Float32Array,Float64Array]) { class A extends C {} let a = new A([1,2]); ok = ok && a instanceof A && a instanceof C && a.length === 2 && a[1] === 2; } ok",
    );
    check(
        "class L extends Intl.Locale {} let l = new L('en-US'); l instanceof L && l instanceof Intl.Locale && l.toString() === 'en-US'",
    );
    check(
        "let ok = true; for (let C of [Intl.NumberFormat,Intl.PluralRules,Intl.Collator,Intl.DateTimeFormat,Intl.ListFormat,Intl.RelativeTimeFormat,Intl.Segmenter,Intl.DurationFormat]) { class X extends C {} let x = new X('en'); ok = ok && x instanceof X && x instanceof C && Object.getPrototypeOf(x) === X.prototype; } ok",
    );
    check(
        "class D extends Intl.DisplayNames {} let d = new D('en',{type:'language'}); d instanceof D && d.of('en') === 'English'",
    );
}

#[test]
fn constructor_callbacks_resume_after_dependency_wait_and_migration() {
    use clj_quickjs::{Completion, Primitive};
    for source in [
        "class N extends Number {} let n = new N({valueOf(){return __rt.need(309,1);}}); n instanceof N && n.valueOf() === 8",
        "class E extends Error {} let e = new E('x',{get cause(){return __rt.need(309,1);}}); e instanceof E && e.cause === 8 && e.message === 'x'",
        "class M extends Map {set(k,v){return super.set(k,__rt.need(309,1));}} let m = new M([[1,2]]); m instanceof M && m.get(1) === 8",
        "class P extends Promise {} let p = new P(resolve => resolve(__rt.need(309,1))); p instanceof P && p instanceof Promise",
    ] {
        let runtime = Runtime::new();
        let code = compile_script("constructor-wait.js", source).unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        runtime.collect().unwrap();
        let request = task.take_producer_requests().pop().unwrap();
        let ready = runtime.root(Primitive::Number(8.));
        let mut runnable = runtime
            .complete(request.key(), request.producer(), Completion::Ready(&ready))
            .unwrap()
            .into_runnable();
        let task = runnable.pop().unwrap();
        let result = std::thread::spawn(move || task.run().unwrap())
            .join()
            .unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{source}"
        );
    }
}

#[test]
fn builtin_subclasses_run_on_small_worker_stacks() {
    for source in [
        "class A extends Array {} new A(1,2).length === 2",
        "class E extends Error {} new E({toString(){return 'x';}}).message === 'x'",
        "class M extends Map {set(k,v){return super.set(k,v+1);}} new M([[1,2]]).get(1) === 3",
        "class N extends Number {} new N({valueOf(){return 7;}}).valueOf() === 7",
        "class R extends RegExp {} new R({toString(){return 'a';}}).test('a')",
        "class U extends Uint8Array {} new U([1,2])[1] === 2",
        "class F extends Function {} new F('return 7')() === 7",
        "class P extends Promise {} new P(resolve => resolve(7)) instanceof P",
        "class D extends Date {} new D({valueOf(){return 7;}}) instanceof D",
        "class B extends ArrayBuffer {} new B({valueOf(){return 8;}}) instanceof B",
        "class I extends Intl.NumberFormat {} new I('en').format(12) === '12'",
    ] {
        let runtime = Runtime::new();
        runtime
            .spawn(&compile_script("init", "Object").unwrap(), &[])
            .unwrap()
            .run()
            .unwrap();
        let code = compile_script("small-constructor.js", source).unwrap();
        let task = runtime.spawn(&code, &[]).unwrap();
        let result = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || task.run().unwrap())
            .unwrap()
            .join()
            .unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{source}"
        );
    }
}
