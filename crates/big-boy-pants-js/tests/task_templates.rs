use big_boy_pants_js::frontend::{compile_function, compile_script};
use big_boy_pants_js::{
    Completion, DependencyId, DependencyKey, Primitive, ResultKind, RunOutcome, Runtime, Task,
    ValueView,
};

fn complete(rt: &Runtime, task: &Task, value: f64) -> RunOutcome {
    let request = task
        .take_producer_requests()
        .pop()
        .expect("new command request");
    let value = rt.root(Primitive::Number(value));
    let _wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    task.run().unwrap()
}

#[test]
fn template_copies_closures_cycles_descriptors_map_keys_and_globals() {
    let rt = Runtime::new();
    let exe = compile_script("template.js", r#"
        let n = 0;
        const object = { value: 2 }; object.self = object;
        const bump = () => ++n;
        const bound = bump.bind(object);
        const map = new Map([[object, object]]), set = new Set([object]);
        const array = [object, bump]; array.length = 5;
        Object.defineProperty(array, '2', {get: bump, configurable: false});
        const proxy = new Proxy(object, {get: (target, key) => target[key]});
        globalThis.saved = object;
        __rt.need(1, 1);
        if (object.self !== object || map.get(object) !== object || !set.has(object) ||
            array[0] !== object || saved !== object || proxy.self !== object ||
            array.length !== 5 || 3 in array || Object.getOwnPropertyDescriptor(array, '2').configurable)
            throw Error('graph or descriptor changed');
        bump() + bound() + array[2];
    "#).unwrap();
    let source = rt.spawn(&exe, &[]).unwrap();
    assert!(matches!(source.run().unwrap(), RunOutcome::Waiting(_)));
    let template = rt.task_template(&source, &[]).unwrap();
    assert!(
        matches!(complete(&rt, &source, 0.0), RunOutcome::Complete(v) if v.view() == ValueView::Number(6.0))
    );
    drop(source);
    rt.collect().unwrap();
    std::thread::scope(|scope| {
        let mut threads = Vec::new();
        for kind in 2..10 {
            let (rt, template) = (&rt, &template);
            threads.push(scope.spawn(move || {
                let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(kind));
                let task = rt.spawn_template(template, &[], key).unwrap();
                assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key));
                assert!(matches!(complete(rt, &task, 0.0), RunOutcome::Complete(v) if v.view() == ValueView::Number(6.0)));
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
    });
    rt.collect().unwrap();
}

#[test]
fn template_replaces_parameter_identity_through_closures_and_persistent_vectors() {
    let rt = Runtime::new();
    let parameter = |n: f64| {
        rt.scope(|s| {
            let n = s.primitive(Primitive::Number(n));
            let private = s.new_object();
            s.persist(s.new_vector(&[n, private]).unwrap())
        })
    };
    let old = parameter(1.0);
    let exe = compile_function(r#"function main(config) {
        const object = {}, wrapped = __rt.vector(object, config);
        object.wrapped = wrapped;
        const kind = () => __rt.nth(config, 0);
        __rt.need(1, kind());
        if (__rt.nth(wrapped, 0) !== object || __rt.nth(wrapped, 1) !== config || object.wrapped !== wrapped) throw Error('alias');
        __rt.need(2, kind());
        return kind();
    }"#).unwrap();
    let source = rt.spawn(&exe, &[&old]).unwrap();
    assert!(matches!(source.run().unwrap(), RunOutcome::Waiting(_)));
    let template = rt.task_template(&source, &[&old]).unwrap();
    let new = parameter(9.0);
    let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(9));
    let task = rt.spawn_template(&template, &[&new], key).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key));
    assert!(
        matches!(complete(&rt, &task, 0.0), RunOutcome::Waiting(k) if k.kind() == ResultKind::new(9) && k.dependency() == DependencyId::new(2))
    );
    rt.collect().unwrap();
    assert!(
        matches!(complete(&rt, &task, 0.0), RunOutcome::Complete(v) if v.view() == ValueView::Number(9.0))
    );
}

#[test]
fn template_rejects_unsupported_state_without_running_getters() {
    for source in [
        "const map = new WeakMap(), key = {}; map.set(key, 1); __rt.need(1, 1); map.get(key);",
        "const date = new Date(0); __rt.need(1, 1); date;",
        "function f() { __rt.need(1, 1); } f();",
    ] {
        let rt = Runtime::new();
        let exe = compile_script("unsupported.js", source).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        assert!(rt.task_template(&task, &[]).is_err(), "{source}");
    }
}

#[test]
fn template_does_not_publish_private_functions_inside_persistent_vectors() {
    let rt = Runtime::new();
    let exe = compile_script(
        "private-function.js",
        r#"
        const fn = () => 7, wrapped = __rt.vector(fn);
        const command = __rt.need(1, 1);
        if (command === 9) fn.poison = true;
        else if (fn.poison !== undefined) throw Error('source mutation reached checkpoint');
        __rt.nth(wrapped, 0) === fn && fn() === 7;
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let template = rt.task_template(&task, &[]).unwrap();
    assert!(
        matches!(complete(&rt, &task, 9.0), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
    drop(task);
    rt.collect().unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(2));
    let copy = rt.spawn_template(&template, &[], key).unwrap();
    assert!(matches!(copy.run().unwrap(), RunOutcome::Waiting(_)));
    assert!(
        matches!(complete(&rt, &copy, 0.0), RunOutcome::Complete(v) if v.view() == ValueView::Bool(true))
    );
}

#[test]
fn template_allocation_limits_fail_cleanly_and_collection_allows_retry() {
    let rt = Runtime::try_with_heap_policy(
        Default::default(),
        big_boy_pants_js::HeapPolicy {
            hard_limit_objects: Some(5000),
            hard_limit_bytes: Some(32 * 1024 * 1024),
            ..Default::default()
        },
    )
    .unwrap();
    let exe = compile_script(
        "bounded-template.js",
        "const a=[]; for(let i=0;i<80;i++) a.push({i}); __rt.need(1,1); a.length;",
    )
    .unwrap();
    let source = rt.spawn(&exe, &[]).unwrap();
    assert!(matches!(source.run().unwrap(), RunOutcome::Waiting(_)));
    let template = rt.task_template(&source, &[]).unwrap();
    drop(source);
    rt.collect().unwrap();
    let mut copies = Vec::new();
    let mut limited = false;
    for kind in 2..100 {
        let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(kind));
        match rt.spawn_template(&template, &[], key) {
            Ok(task) => copies.push(task),
            Err(_) => {
                limited = true;
                break;
            }
        }
    }
    assert!(limited && !copies.is_empty());
    drop(copies);
    rt.collect().unwrap();
    let key = DependencyKey::new(rt.world(), DependencyId::new(1), ResultKind::new(200));
    let task = rt.spawn_template(&template, &[], key).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    assert!(
        matches!(complete(&rt, &task, 0.0), RunOutcome::Complete(v) if v.view() == ValueView::Number(80.0))
    );
}
