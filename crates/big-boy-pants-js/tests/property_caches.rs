//! Named access fixtures exercise one instruction repeatedly as layouts and
//! descriptors change. These are synthetic JavaScript, not compiler output.
use big_boy_pants_js::frontend::{compile_function, compile_script};
use big_boy_pants_js::{Budget, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView};

fn check(source: &str) {
    let runtime = Runtime::new();
    let code = compile_script("cache.js", source).unwrap();
    let outcome = runtime.spawn(&code, &[]).unwrap().run().unwrap();
    let RunOutcome::Complete(value) = outcome else {
        panic!("{source}: {outcome:?}")
    };
    assert_eq!(value.view(), ValueView::Bool(true), "{source}");
}

#[test]
fn named_cache_tracks_shapes_deletion_and_order() {
    check(
        "function read(o) { return o.x; } function write(o,v) { o.x = v; } let a = {x:1}, b = {y:2,x:3}; let ok = read(a) === 1 && read(b) === 3; a.y = 4; write(a,5); ok = ok && read(a) === 5; delete b.y; delete b.x; b.z = 6; b.x = 7; write(b,8); ok && read(b) === 8 && Object.keys(b).join(',') === 'z,x'",
    );
    check(
        "function read(o) { return o.x; } let a = {x:1}; let ok = read(a) === 1; for (let i=0;i<80;i++) a['k'+i]=i; a.x=9; ok && read(a) === 9 && Object.keys(a).length === 81",
    );
}

#[test]
fn named_cache_tracks_prototypes_and_method_replacement() {
    check(
        "function read(o) { return o.x; } function call(o) { return o.m(); } let p = {x:1,m(){return this.x;}}, q = {x:2,m(){return this.x+10;}}, o = Object.create(p); let ok = read(o) === 1 && call(o) === 1; p.x=3; ok = ok && read(o) === 3; Object.setPrototypeOf(o,q); ok = ok && read(o) === 2 && call(o) === 12; q.m=function(){return 99;}; o.x=7; ok && read(o) === 7 && call(o) === 99",
    );
}

#[test]
fn named_cache_reads_current_descriptors_and_setters() {
    check(
        "function read(o) { return o.x; } function write(o,v) { o.x=v; } let o={x:1}, n=0; let ok=read(o)===1; Object.defineProperty(o,'x',{get(){return this.y;},set(v){n++;this.y=v;},configurable:true}); write(o,7); ok=ok && read(o)===7 && n===1; Object.defineProperty(o,'x',{value:8,writable:true}); write(o,9); ok=ok && read(o)===9; Object.freeze(o); try {write(o,10);} catch(e) {ok=ok && e instanceof TypeError;} ok && read(o)===9",
    );
    check(
        "function write(o,v) {o.x=v;} let p={x:1}, o=Object.create(p); write(o,2); delete o.x; Object.defineProperty(p,'x',{set(v){this.y=v;},get(){return this.y;}}); write(o,5); o.x===5 && o.y===5 && Object.getOwnPropertyDescriptor(o,'x')===undefined",
    );
}

#[test]
fn concurrent_tasks_share_executable_with_different_shapes() {
    let runtime = Runtime::new();
    let code = compile_function("function run(which) { let o=which ? {x:1,z:2} : {z:2,x:1}; function get(o){return o.x;} function set(o,v){o.x=v;} let sum=0; for(let i=0;i<2000;i++){set(o,i);sum+=get(o);} return sum; }").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let workers: Vec<_> = [false, true]
            .into_iter()
            .map(|which| {
                let argument = runtime.root(Primitive::Bool(which));
                let task = runtime.spawn(&code, &[&argument]).unwrap();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let RunOutcome::Complete(value) = task.run().unwrap() else {
                        panic!("complete")
                    };
                    assert_eq!(value.view(), ValueView::Number(1_999_000.0));
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
}

#[test]
fn concurrent_runtimes_do_not_confuse_interned_key_bindings() {
    let code = compile_function("function run() { let o={x:1,y:2}; let sum=0; for(let i=0;i<2000;i++){o.x=i;sum+=o.x+o.y;} return sum; }").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let workers: Vec<_> = ["({a:1,b:2,c:3})", "({z:1})"]
            .into_iter()
            .map(|source| {
                let runtime = Runtime::new();
                let setup = compile_script("different-keys.js", source).unwrap();
                // Retain the setup task so the two heaps assign different key IDs.
                let setup_task = runtime.spawn(&setup, &[]).unwrap();
                setup_task.run().unwrap();
                let task = runtime.spawn(&code, &[]).unwrap();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let RunOutcome::Complete(value) = task.run().unwrap() else {
                        panic!("complete")
                    };
                    assert_eq!(value.view(), ValueView::Number(2_003_000.0));
                    drop(setup_task);
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
}

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        panic!("no effects")
    }
}

#[test]
fn cache_keys_rebind_across_collection_and_runtimes() {
    let code = compile_function("function run() { let o={uniqueCacheKey:3}; let sum=0; for(let i=0;i<20;i++) {o.uniqueCacheKey++;sum+=o.uniqueCacheKey;} return sum; }").unwrap();
    for _ in 0..3 {
        let runtime = Runtime::new();
        for _ in 0..2 {
            let task = runtime.spawn(&code, &[]).unwrap();
            loop {
                match task
                    .run_with(Budget::instructions(7), &mut NoEffects)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        runtime.collect().unwrap();
                    }
                    RunOutcome::Complete(value) => {
                        assert_eq!(value.view(), ValueView::Number(270.0));
                        break;
                    }
                    other => panic!("{other:?}"),
                }
            }
            drop(task);
            runtime.collect().unwrap();
            // Reuse freed key slots for unrelated names before running the
            // same executable, whose non-rooting atomic hints survived GC.
            let other = compile_script("reuse.js", "({unrelated:1,other:2}).other").unwrap();
            runtime.spawn(&other, &[]).unwrap().run().unwrap();
        }
    }
}
