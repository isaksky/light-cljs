//! Synthetic JavaScript fixtures, not light-cljs output.

use big_boy_pants_js::frontend::compile_script;
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
fn promise_subclasses_support_private_elements() {
    check(
        "class P extends Promise { #x = 1; getX() { return this.#x; } static has(p) { return #x in p; } } let p = new P(() => {}); p.getX() === 1 && P.has(p) && !P.has(new Promise(() => {}))",
    );
    check(
        r#"
        class P extends Promise {
            #payload = {value: 7};
            #method() { return this.#payload.value; }
            get #x() { return this.#method(); }
            set #x(value) { this.#payload = {value}; }
            get x() { return this.#x; }
            set x(value) { this.#x = value; }
            static has(p) { return #payload in p && #method in p && #x in p; }
        }
        let p = new P(() => {});
        let ok = p.x === 7 && P.has(p) && Reflect.ownKeys(p).length === 0;
        Object.freeze(p);
        p.x = 9;
        let proxy = new Proxy(p, {});
        let caught = false;
        try { P.prototype.x; } catch(e) { caught = e instanceof TypeError; }
        ok && p.x === 9 && caught && !P.has(proxy);
        "#,
    );
    // A returning super can stamp a native Promise without public properties.
    check(
        "let p = Object.freeze(new Promise(() => {})); class B { constructor() { return p; } } class C extends B { #x = {value: 11}; getX() { return this.#x.value; } static read(p) { return p.#x.value; } } new C() === p && C.read(p) === 11 && Reflect.ownKeys(p).length === 0",
    );
}

#[test]
fn promise_private_elements_remain_task_owned_after_publication() {
    for base in ["Promise", "B"] {
        let runtime = Runtime::new();
        let source = format!(
            r#"
            let resolve, p = new Promise(r => {{ resolve = r; }});
            class B {{ constructor() {{ return p; }} }}
            class P extends {base} {{
                #x = {{value: 7}};
                #method() {{ return this.#x.value; }}
                get #accessor() {{ return this.#method(); }}
                static operations() {{ return __rt.vector(
                    o => #x in o, o => o.#x, o => {{ o.#x = 2; }},
                    o => o.#method(), o => o.#accessor);
                }}
            }}
            let instance = new P(r => {{ resolve = r; }});
            __rt.vector(instance, P.operations(), resolve);
            "#
        );
        let code = compile_script("private-promise.js", &source).unwrap();
        let RunOutcome::Complete(pair) = runtime.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("expected Promise and private operations");
        };
        runtime.publish(&pair).unwrap();
        runtime.collect().unwrap();
        let consumer = big_boy_pants_js::frontend::compile_function(
            r#"function consume(pair) {
                let p = __rt.nth(pair, 0), operations = __rt.nth(pair, 1), caught = 0;
                for (let i = 0; i < 5; i++) {
                    try { __rt.nth(operations, i)(p); }
                    catch(e) { if(e instanceof TypeError) caught++; }
                }
                class B { constructor() { return p; } }
                class Stamp extends B { #foreign = 3; }
                try { new Stamp(); } catch(e) { if(e instanceof TypeError) caught++; }
                __rt.nth(pair, 2)(8);
                return caught === 6;
            }"#,
        )
        .unwrap();
        let result = runtime.spawn(&consumer, &[&pair]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{base}"
        );
        runtime.publish(&pair).unwrap();
    }
}

#[test]
fn promise_private_storage_claims_respect_existing_ownership() {
    for (source, can_stamp) in [
        ("new Promise(() => {});", true),
        ("class P extends Promise {} new P(() => {});", false),
    ] {
        let runtime = Runtime::new();
        let code = compile_script("unstamped-promise.js", source).unwrap();
        let RunOutcome::Complete(promise) = runtime.spawn(&code, &[]).unwrap().run().unwrap()
        else {
            panic!("expected Promise");
        };
        runtime.publish(&promise).unwrap();
        let stamp = big_boy_pants_js::frontend::compile_function(
            r#"function stamp(p) {
                class B { constructor() { return p; } }
                class C extends B {
                    #x = {value: 11};
                    static read(o) { return o.#x.value; }
                    static has(o) { return #x in o; }
                    static operations() { return __rt.vector(
                        o => #x in o, o => o.#x, o => { o.#x = 2; }); }
                }
                let absent = false;
                try { absent = !C.has(p); } catch(e) {
                    if (!(e instanceof TypeError)) return false;
                }
                try { new C(); } catch(e) { return e instanceof TypeError && !absent; }
                return __rt.vector(p, C.operations(), absent && C.has(p) && C.read(p) === 11);
            }"#,
        )
        .unwrap();
        let RunOutcome::Complete(result) =
            runtime.spawn(&stamp, &[&promise]).unwrap().run().unwrap()
        else {
            panic!("expected stamping result: {source}");
        };
        if !can_stamp {
            assert_eq!(result.view(), ValueView::Bool(true));
            continue;
        }
        runtime.publish(&result).unwrap();
        runtime.collect().unwrap();
        let foreign = big_boy_pants_js::frontend::compile_function(
            r#"function foreign(pair) {
                let p = __rt.nth(pair, 0), operations = __rt.nth(pair, 1), caught = 0;
                for (let i = 0; i < 3; i++) {
                    try { __rt.nth(operations, i)(p); }
                    catch(e) { if(e instanceof TypeError) caught++; }
                }
                class B { constructor() { return p; } }
                class Stamp extends B { #foreign = 3; }
                try { new Stamp(); } catch(e) { if(e instanceof TypeError) caught++; }
                return __rt.nth(pair, 2) && caught === 4;
            }"#,
        )
        .unwrap();
        assert!(
            matches!(runtime.spawn(&foreign, &[&result]).unwrap().run().unwrap(),
                RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
        );
    }
}

#[test]
fn derived_promise_own_properties_follow_ordinary_object_rules() {
    check(
        r#"
        class P extends Promise { constructor() { super(() => {}); this.tag = {value: 7}; } }
        let p = new P();
        Object.defineProperty(p, 'answer', {get() { return this.tag.value; }, configurable: true});
        let ok = p.answer === 7 && Object.keys(p).join(',') === 'tag' &&
            Object.getOwnPropertyDescriptor(p, 'tag').writable && delete p.answer;
        let prototype = {marker: 8};
        Object.setPrototypeOf(p, prototype);
        ok = ok && p.marker === 8 && Object.getPrototypeOf(p) === prototype;
        Object.freeze(p);
        let caught = false; try { Object.defineProperty(p, 'extra', {value: 1}); }
        catch(e) { caught = e instanceof TypeError; }
        ok && caught && Object.isFrozen(p) && !Object.isExtensible(p) && p.tag.value === 7;
    "#,
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
fn derived_promise_json_snapshot_limits() {
    for (key, enumerable, expected) in [
        ("i", true, "JSON.parse(text)[4095] === 1"),
        ("i", false, "text === '{}'"),
        ("Symbol()", true, "text === '{}'"),
    ] {
        let source = format!(
            r#"
            class P extends Promise {{}}
            let p = new P(() => {{}});
            for (let i = 0; i < 4096; i++) {{
                Object.defineProperty(p, {key}, {{value: 1, enumerable: {enumerable}}});
            }}
            let text = JSON.stringify(p);
            let ok = {expected};
            let i = 4096;
            Object.defineProperty(p, {key}, {{value: 1, enumerable: {enumerable}}});
            let caught = false;
            try {{ JSON.stringify(p); }} catch(e) {{
                caught = e instanceof RangeError && e.message.indexOf('object property limit of 4096') >= 0;
            }}
            ok && caught;
        "#
        );
        let runtime = Runtime::new();
        let code = compile_script("promise-json-limit.js", &source).unwrap();
        let outcome = runtime.spawn(&code, &[]).unwrap().run().unwrap();
        assert!(
            matches!(outcome, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{key}, enumerable: {enumerable}"
        );
    }
}

#[test]
fn derived_promises_publish_without_exposing_mutable_properties() {
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
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return p.constructor;}",
        ),
        (
            "class P extends Promise {} let p = new P(() => {}); p.private = {value: 7}; p;",
            "function consume(p){return p.private;}",
        ),
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){p.private = 1;}",
        ),
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return Object.keys(p);}",
        ),
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return Object.getOwnPropertyDescriptor(p, 'private');}",
        ),
        (
            "class P extends Promise {} new P(() => {});",
            "function consume(p){return Object.freeze(p);}",
        ),
    ] {
        let runtime = Runtime::new();
        let code = compile_script("local-promise.js", source).unwrap();
        let RunOutcome::Complete(value) = runtime.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("expected constructor result");
        };
        runtime.publish(&value).unwrap();
        let consumer = big_boy_pants_js::frontend::compile_function(consumer).unwrap();
        let result = runtime.spawn(&consumer, &[&value]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Failed(e) if matches!(e.kind(), big_boy_pants_js::VmErrorKind::ForeignObject { .. })),
            "{source}"
        );
    }
}

#[test]
fn derived_promise_resolvers_can_be_called_from_another_task() {
    for resolver in ["resolve", "reject"] {
        let runtime = Runtime::new();
        let code = compile_script(
            "derived-resolver.js",
            &format!("let saved; class P extends Promise {{}} new P((resolve, reject) => {{saved = {resolver};}}); saved;"),
        ).unwrap();
        let RunOutcome::Complete(value) = runtime.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("expected resolver");
        };
        let consumer =
            big_boy_pants_js::frontend::compile_function("function settle(r){r(1); return true;}")
                .unwrap();
        let result = runtime.spawn(&consumer, &[&value]).unwrap().run().unwrap();
        assert!(
            matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)),
            "{resolver}"
        );
        runtime.publish(&value).unwrap();
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
    use big_boy_pants_js::{Completion, Primitive};
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

#[test]
fn derived_function_and_promise_prototype_cycles_are_rejected() {
    check(
        "class P extends Promise {} let p = new P(() => {}); let ok = false; try {Object.setPrototypeOf(P.prototype,p);} catch(e){ok = e instanceof TypeError;} ok && p instanceof P",
    );
    check(
        "class F extends Function {} let f = new F; let ok = false; try {Object.setPrototypeOf(F.prototype,f);} catch(e){ok = e instanceof TypeError;} ok && f instanceof F",
    );
}

#[test]
fn collection_adders_preserve_prototype_mutations() {
    check(
        "class M extends Map {set(k,v){super.set(k,v); Object.setPrototypeOf(this,null); return this;}} let m = new M([[1,2]]); Object.getPrototypeOf(m) === null && Map.prototype.get.call(m,1) === 2",
    );
    check(
        "class M extends Map {set(k,v){super.set(k,v); Object.setPrototypeOf(this,null); Object.preventExtensions(this); return this;}} let m = new M([[1,2]]); Object.getPrototypeOf(m) === null && !Object.isExtensible(m) && Map.prototype.get.call(m,1) === 2",
    );
    check(
        "class S extends Set {add(k){super.add(k); Object.setPrototypeOf(this,null); Object.preventExtensions(this); return this;}} let s = new S([1]); Object.getPrototypeOf(s) === null && Set.prototype.has.call(s,1)",
    );
    check(
        "class W extends WeakMap {set(k,v){super.set(k,v); Object.setPrototypeOf(this,null); Object.preventExtensions(this); return this;}} let k = {}; let w = new W([[k,2]]); Object.getPrototypeOf(w) === null && WeakMap.prototype.get.call(w,k) === 2",
    );
    check(
        "class W extends WeakSet {add(k){super.add(k); Object.setPrototypeOf(this,null); Object.preventExtensions(this); return this;}} let k = {}; let w = new W([k]); Object.getPrototypeOf(w) === null && WeakSet.prototype.has.call(w,k)",
    );
}

#[test]
fn typed_array_primitive_length_precedes_prototype_lookup() {
    check(
        "let n = 0; let B = Uint8Array.bind(null); Object.defineProperty(B,'prototype',{get(){n++; throw 17;}}); let C = new Proxy(B,{}); let ok; try {new C(-1);} catch(e){ok = e instanceof RangeError;} ok && n === 0",
    );
    check(
        "let B = Uint8Array.bind(null); Object.defineProperty(B,'prototype',{get(){throw 17;}}); let C = new Proxy(B,{}); let caught; try {new C(3);} catch(e){caught = e;} caught === 17",
    );
    check(
        "let n = 0; let B = Uint8Array.bind(null); Object.defineProperty(B,'prototype',{get(){n++; throw 17;}}); let C = new Proxy(B,{}); let caught; try {new C({get length(){throw 23;}});} catch(e){caught = e;} caught === 17 && n === 1",
    );
}

#[test]
fn regexp_new_target_protocol_order_and_waits() {
    let source = "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return RegExp.prototype;}}); let pattern = {get [Symbol.match](){log += 'm'; return true;}, get source(){log += 's'; return 'a';}, get flags(){log += 'f'; return 'g';}}; Reflect.construct(RegExp,[pattern],B); log";
    let runtime = Runtime::new();
    let code = compile_script("regexp-order.js", source).unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(value) = task.run().unwrap() else {
        panic!("RegExp construction failed");
    };
    assert_eq!(value.string().unwrap().to_string(), "msfp");

    // Present Proxy get traps are outside the supported surface. Forwarding
    // proxies and an ownKeys trap called by a getter still record the order.
    check(
        "let log = ''; let proto = Object.create(RegExp.prototype); let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return proto;}}); let pattern = {get [Symbol.match](){log += 'm'; return true;}, get constructor(){throw 99;}, get source(){log += 's'; return {toString(){log += 'x'; return 'a';}};}, get flags(){log += 'f'; proto = Object.create(RegExp.prototype); return {toString(){log += 'y'; return 'g';}};}}; let r = Reflect.construct(RegExp,[pattern],B); log === 'msfpxy' && Object.getPrototypeOf(r) === proto && r.source === 'a' && r.flags === 'g'",
    );
    check(
        "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return RegExp.prototype;}}); let N = new Proxy(B,{}); let probe = new Proxy({}, {ownKeys(){log += 'k'; return [];}}); let pattern = new Proxy({get [Symbol.match](){log += 'm'; Reflect.ownKeys(probe); return true;}, get source(){log += 's'; return 'a';}, get flags(){log += 'f'; return 'g';}},{}); let r = Reflect.construct(RegExp,[pattern],N); log === 'mksfp' && r.source === 'a' && r.flags === 'g'",
    );
    check(
        "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return null;}}); let pattern = {get [Symbol.match](){log += 'm'; return true;}, get source(){log += 's'; return 'a';}, get flags(){throw 99;}}; let r = Reflect.construct(RegExp,[pattern,{toString(){log += 'y'; return 'i';}}],B); log === 'mspy' && Object.getPrototypeOf(r) === RegExp.prototype && r.flags === 'i'",
    );
    check(
        "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return RegExp.prototype;}}); let pattern = {get [Symbol.match](){log += 'm'; return false;}, get source(){throw 99;}, get flags(){throw 99;}, toString(){log += 'x'; return 'a';}}; Reflect.construct(RegExp,[pattern],B); log === 'mpx'",
    );
    check(
        "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; return RegExp.prototype;}}); let pattern = /a/g; Object.defineProperty(pattern,Symbol.match,{get(){log += 'm'; return false;}}); Object.defineProperty(pattern,'source',{get(){throw 99;}}); Object.defineProperty(pattern,'flags',{get(){throw 99;}}); let r = Reflect.construct(RegExp,[pattern],B); log === 'mp' && r !== pattern && r.source === 'a' && r.flags === 'g'",
    );
    check(
        "let ok = true; for (let stop of ['m','s','f','p','x','y']) {let log = ''; function hit(s){log += s; if(s === stop) throw stop;} let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){hit('p'); return RegExp.prototype;}}); let pattern = {get [Symbol.match](){hit('m'); return true;}, get source(){hit('s'); return {toString(){hit('x'); return 'a';}};}, get flags(){hit('f'); return {toString(){hit('y'); return 'g';}};}}; let caught; try {Reflect.construct(RegExp,[pattern],B);} catch(e){caught = e;} ok = ok && caught === stop && log === 'msfpxy'.slice(0,'msfpxy'.indexOf(stop)+1);} ok",
    );
    check(
        "let log = ''; let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){log += 'p'; throw 17;}}); let caught; try {Reflect.construct(RegExp,['['],B);} catch(e){caught = e;} caught === 17 && log === 'p'",
    );
    // Calling RegExp still observes constructor before source/flags, and keeps
    // the identity fast path. Constructing it must never read constructor.
    check(
        "let log = ''; let pattern = new Proxy({get [Symbol.match](){log += 'm'; return true;}, get constructor(){log += 'c'; return RegExp;}, get source(){throw 99;}},{}); RegExp(pattern) === pattern && log === 'mc'",
    );
    check(
        "let log = ''; let pattern = {get [Symbol.match](){log += 'm'; return true;}, get constructor(){log += 'c'; return null;}, get source(){log += 's'; return 'a';}, get flags(){log += 'f'; return 'g';}}; let r = RegExp(pattern); log === 'mcsf' && r.source === 'a' && r.flags === 'g'",
    );

    let source = "let log = ''; function hit(s,n){log += s; __rt.need(327,n);} let B = RegExp.bind(null); Object.defineProperty(B,'prototype',{get(){hit('p',4); let proto = Object.create(RegExp.prototype); proto.tag = 7; return proto;}}); let pattern = {get [Symbol.match](){hit('m',1); return true;}, get constructor(){throw 99;}, get source(){hit('s',2); return {toString(){hit('x',5); return 'a';}};}, get flags(){hit('f',3); return {toString(){hit('y',6); return 'g';}};}}; let r = Reflect.construct(RegExp,[pattern],B); log === 'msfpxy' && r.tag === 7 && r.source === 'a' && r.flags === 'g'";
    let runtime = Runtime::new();
    let code = compile_script("regexp-order-wait.js", source).unwrap();
    let mut task = runtime.spawn(&code, &[]).unwrap();
    for _ in 0..6 {
        let (resumed, outcome) = std::thread::spawn(move || {
            let outcome = task.run().unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = resumed;
        assert!(matches!(outcome, RunOutcome::Waiting(_)));
        runtime.collect().unwrap();
        let mut requests = task.take_producer_requests();
        assert_eq!(requests.len(), 1);
        let request = requests.pop().unwrap();
        let ready = runtime.root(big_boy_pants_js::Primitive::Undefined);
        let mut runnable = runtime
            .complete(
                request.key(),
                request.producer(),
                big_boy_pants_js::Completion::Ready(&ready),
            )
            .unwrap()
            .into_runnable();
        assert_eq!(runnable.len(), 1);
        task = runnable.pop().unwrap();
    }
    let result = std::thread::spawn(move || task.run().unwrap())
        .join()
        .unwrap();
    assert!(matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}

#[test]
fn new_target_prototype_getter_resumes_once_and_propagates_errors() {
    check(
        "let n = 0; let B = Array.bind(null); Object.defineProperty(B,'prototype',{get(){n++; throw 17;}}); let C = new Proxy(B,{}); let caught; try {new C(1,2);} catch(e){caught = e;} caught === 17 && n === 1",
    );
    check(
        "let n = 0; let B = Array.bind(null); Object.defineProperty(B,'prototype',{get(){n++; return null;}}); let C = new Proxy(B,{}); let a = new C(1,2); Array.isArray(a) && a.length === 2 && Object.getPrototypeOf(a) === Array.prototype && n === 1",
    );
    let runtime = Runtime::new();
    let code = compile_script("prototype-wait.js", "let reads = 0; let proto = Object.create(Array.prototype); let B = Array.bind(null,1); Object.defineProperty(B,'prototype',{get(){reads++; __rt.need(309,1); return proto;}}); let C = new Proxy(B,{}); let a = new C(2,3); reads === 1 && Object.getPrototypeOf(a) === proto && Array.isArray(a) && a.length === 3 && a[0] === 1 && a[2] === 3").unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    runtime.collect().unwrap();
    let request = task.take_producer_requests().pop().unwrap();
    let ready = runtime.root(big_boy_pants_js::Primitive::Undefined);
    let mut runnable = runtime
        .complete(
            request.key(),
            request.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap()
        .into_runnable();
    let task = runnable.pop().unwrap();
    let result = std::thread::spawn(move || task.run().unwrap())
        .join()
        .unwrap();
    assert!(matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}
