//! Synthetic JavaScript fixtures, not light-cljs output (clj-quickjs-311).
use big_boy_pants_js::{
    Budget, Completion, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
    frontend::compile_script,
};

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn check(source: &str) {
    let rt = Runtime::new();
    let code = compile_script("object-to-string.js", source).unwrap();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{source}: {result:?}"
    );
}

#[test]
fn builtin_and_intrinsic_tags() {
    for (value, tag) in [
        ("undefined", "Undefined"),
        ("null", "Null"),
        ("true", "Boolean"),
        ("1", "Number"),
        ("'abc'", "String"),
        ("Symbol()", "Symbol"),
        ("1n", "BigInt"),
        ("{}", "Object"),
        ("[]", "Array"),
        ("Array.prototype", "Array"),
        ("function() {}", "Function"),
        ("new Error()", "Error"),
        ("new SuppressedError(1, 2)", "Error"),
        ("new Date(0)", "Date"),
        ("/x/", "RegExp"),
        ("new Boolean(false)", "Boolean"),
        ("new Number(0)", "Number"),
        ("new String('')", "String"),
        ("(function() { return arguments; })(1)", "Arguments"),
        ("Math", "Math"),
        ("JSON", "JSON"),
        ("Reflect", "Reflect"),
        ("Promise.resolve(1)", "Promise"),
        ("new Map()", "Map"),
        ("new Set()", "Set"),
        ("new WeakMap()", "WeakMap"),
        ("new WeakSet()", "WeakSet"),
        ("Object(Symbol())", "Symbol"),
        ("Object(1n)", "BigInt"),
        ("new ArrayBuffer(0)", "ArrayBuffer"),
        ("new Uint8Array(0)", "Uint8Array"),
        ("(function*() {})()", "Generator"),
        ("(async function*() {})()", "AsyncGenerator"),
        ("[].values()", "Array Iterator"),
        ("''[Symbol.iterator]()", "String Iterator"),
        ("new Set().values()", "Set Iterator"),
        ("new DisposableStack()", "DisposableStack"),
        ("new AsyncDisposableStack()", "AsyncDisposableStack"),
        ("Intl", "Intl"),
        ("new Intl.Locale('en')", "Intl.Locale"),
        ("new Intl.NumberFormat('en')", "Intl.NumberFormat"),
        ("new Intl.PluralRules('en')", "Intl.PluralRules"),
        ("new Intl.Collator('en')", "Intl.Collator"),
        ("new Intl.DateTimeFormat('en')", "Intl.DateTimeFormat"),
        ("new Intl.ListFormat('en')", "Intl.ListFormat"),
        (
            "new Intl.RelativeTimeFormat('en')",
            "Intl.RelativeTimeFormat",
        ),
        (
            "new Intl.DisplayNames('en', {type: 'language'})",
            "Intl.DisplayNames",
        ),
        ("new Intl.Segmenter('en')", "Intl.Segmenter"),
        ("new Intl.DurationFormat('en')", "Intl.DurationFormat"),
        (
            "new Intl.Segmenter('en').segment('a')[Symbol.iterator]()",
            "Segmenter String Iterator",
        ),
        ("'a'.matchAll(/a/g)", "RegExp String Iterator"),
        ("new WeakRef({})", "WeakRef"),
        (
            "new FinalizationRegistry(function() {})",
            "FinalizationRegistry",
        ),
    ] {
        check(&format!(
            "Object.prototype.toString.call({value}) === '[object {tag}]'"
        ));
    }
}

#[test]
fn user_tags_and_proxy_dispatch() {
    check(
        "let o = {[Symbol.toStringTag]: 'Custom'}; Object.prototype.toString.call(o) === '[object Custom]'",
    );
    check(
        "let calls = 0, o = Object.create({get [Symbol.toStringTag]() { calls++; return this.x; }}); o.x = 'Inherited'; Object.prototype.toString.call(o) === '[object Inherited]' && calls === 1",
    );
    check(
        "let o = {[Symbol.toStringTag]: {toString() {throw 42;}}}; Object.prototype.toString.call(o) === '[object Object]'",
    );
    check(
        r"let o = {[Symbol.toStringTag]: '\ud800'}; Object.prototype.toString.call(o) === '[object \ud800]'",
    );
    check(
        "let n = 0, a = []; Object.defineProperty(a, Symbol.toStringTag, {get() {n++; return Array.isArray(this) ? 'ProxyArray' : 'Wrong';}}); Object.prototype.toString.call(new Proxy(a, {})) === '[object ProxyArray]' && n === 1",
    );
    check("Object.prototype.toString.call(new Proxy(new Proxy([], {}), {})) === '[object Array]'");
    check("Object.prototype.toString.call(new Proxy(function() {}, {})) === '[object Function]'");
    check("Object.prototype.toString.call(new Proxy(new Error(), {})) === '[object Object]'");
    check(
        "let a = []; a.join = 0; Object.defineProperty(a, Symbol.toStringTag, {get() {return 'Fallback';}}); Array.prototype.toString.call(a) === '[object Fallback]'",
    );
    check(
        "let caught = false; try {Object.prototype.toString.call({get [Symbol.toStringTag]() {throw 42;}});} catch(e) {caught = e === 42;} caught",
    );
    check(
        "let a = (function() {return arguments;})(1); delete a.length; Object.setPrototypeOf(a, null); Object.prototype.toString.call(a) === '[object Arguments]'",
    );
    check(
        "let o = new Map(); Object.defineProperty(o, Symbol.toStringTag, {get() {Object.setPrototypeOf(o, null); return undefined;}}); Object.prototype.toString.call(o) === '[object Object]'",
    );
    for (value, tag) in [
        ("[]", "Array"),
        ("new Error()", "Error"),
        ("new Number(1)", "Number"),
        ("new Boolean(true)", "Boolean"),
        ("new String('x')", "String"),
        ("new Date(0)", "Date"),
        ("/x/", "RegExp"),
        ("(function() {return arguments;})()", "Arguments"),
        ("new Map()", "Object"),
        ("new ArrayBuffer(0)", "Object"),
        ("new Uint8Array(0)", "Object"),
    ] {
        check(&format!(
            "let o = {value}; Object.defineProperty(o, Symbol.toStringTag, {{value: 1}}); Object.prototype.toString.call(o) === '[object {tag}]'"
        ));
    }
}

#[test]
fn intrinsic_tag_descriptors_and_fallbacks() {
    check(
        "let keys = Reflect.ownKeys(Reflect), symbols = 0; for (let key of keys) {if (typeof key === 'symbol') symbols++;} keys.length === 14 && symbols === 1 && keys[13] === Symbol.toStringTag",
    );
    for (prototype, tag) in [
        ("Math", "Math"),
        ("JSON", "JSON"),
        ("Reflect", "Reflect"),
        ("Promise.prototype", "Promise"),
        ("Map.prototype", "Map"),
        ("Set.prototype", "Set"),
        ("WeakMap.prototype", "WeakMap"),
        ("WeakSet.prototype", "WeakSet"),
        ("Symbol.prototype", "Symbol"),
        ("BigInt.prototype", "BigInt"),
        ("ArrayBuffer.prototype", "ArrayBuffer"),
        ("DisposableStack.prototype", "DisposableStack"),
        ("AsyncDisposableStack.prototype", "AsyncDisposableStack"),
        (
            "Object.getPrototypeOf((function*() {}).prototype)",
            "Generator",
        ),
        (
            "Object.getPrototypeOf((async function*() {}).prototype)",
            "AsyncGenerator",
        ),
    ] {
        check(&format!(
            "let p = {prototype}, d = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); d.value === '{tag}' && !d.writable && !d.enumerable && d.configurable && Object.prototype.toString.call(p) === '[object {tag}]'"
        ));
    }
    check(
        "let p = Object.getPrototypeOf(Uint8Array.prototype), d = Object.getOwnPropertyDescriptor(p, Symbol.toStringTag); typeof d.get === 'function' && d.get.name === 'get [Symbol.toStringTag]' && d.get.length === 0 && d.set === undefined && !d.enumerable && d.configurable && d.get.call({}) === undefined && d.get.call(1) === undefined && p[Symbol.toStringTag] === undefined && Uint8Array.prototype[Symbol.toStringTag] === undefined && Object.prototype.toString.call(Uint8Array.prototype) === '[object Object]'",
    );
    for kind in [
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float32Array",
        "Float64Array",
        "Float16Array",
    ] {
        check(&format!(
            "let a = new {kind}(0); a[Symbol.toStringTag] === '{kind}' && Object.prototype.toString.call(a) === '[object {kind}]'"
        ));
    }
    for value in [
        "new Map()",
        "new Set()",
        "new ArrayBuffer(0)",
        "new Uint8Array(0)",
        "Object(Symbol())",
        "Object(1n)",
    ] {
        check(&format!(
            "let o = {value}; Object.setPrototypeOf(o, null); Object.prototype.toString.call(o) === '[object Object]'"
        ));
    }
    check("SuppressedError.prototype[Symbol.toStringTag] === undefined");
}

#[test]
fn tag_receiver_native_root_survives_gc() {
    let rt = Runtime::new();
    let code = compile_script(
        "tag-native-root.js",
        "Object.prototype.toString.call({[Symbol.toStringTag]: 'Ephemeral'})",
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    for _ in 0..500 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.string().unwrap().to_string(), "[object Ephemeral]");
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("inline tag receiver did not complete");
}

#[test]
fn tag_getter_waits_yields_migrates_and_survives_gc() {
    let rt = Runtime::new();
    let code = compile_script("tag-wait.js", "let n = 0; let o = {get [Symbol.toStringTag]() {n++; return __rt.need(311, 1);}}; Object.prototype.toString.call(o) === '[object Ready]' && n === 1").unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    let request = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::String("Ready".into()));
    task = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    for _ in 0..500 {
        let (next, result) = std::thread::spawn(move || {
            let result = task
                .run_with(Budget::instructions(1), &mut NoEffects)
                .unwrap();
            (task, result)
        })
        .join()
        .unwrap();
        task = next;
        match result {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("tag getter did not complete");
}

#[test]
fn tags_initialize_and_execute_on_small_worker_stack() {
    let rt = Runtime::new();
    let code = compile_script("tag-small-stack.js", "let tag = Object.prototype.toString; tag.call({get [Symbol.toStringTag]() {return 'Override';}}) === '[object Override]' && tag.call(new Uint8Array(0)) === '[object Uint8Array]' && tag.call(JSON) === '[object JSON]'").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = std::thread::Builder::new().stack_size(64 * 1024).spawn(move || {
        let result = task.run().unwrap();
        assert!(matches!(result, RunOutcome::Complete(ref value) if value.view() == ValueView::Bool(true)), "{result:?}");
    }).unwrap().join();
    assert!(result.is_ok());
}
