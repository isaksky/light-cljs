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
    let code = compile_script("reflect.js", source).unwrap();
    let runtime = Runtime::new();
    let task = runtime.spawn(&code, &[]).unwrap();
    for _ in 0..100_000 {
        match task.run_with(Budget::instructions(3), &mut NoHost).unwrap() {
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
    panic!("instruction bound: {source}");
}
#[test]
fn namespace_and_existing_gap() {
    check(
        "Object.getOwnPropertyNames(Reflect).sort().join() === 'apply,construct,defineProperty,deleteProperty,get,getOwnPropertyDescriptor,getPrototypeOf,has,isExtensible,ownKeys,preventExtensions,set,setPrototypeOf'",
    );
}
#[test]
fn ordinary_methods_and_failure_results() {
    check(
        "let p = {inherited:1}, o = Object.create(p), s = Symbol('s'); o.a = 3; o[s] = 4; let keys = Reflect.ownKeys(o); Reflect.getPrototypeOf(o) === p && Reflect.has(o,'inherited') && Reflect.get(o,'a') === 3 && keys.length === 2 && keys[0] === 'a' && keys[1] === s && Reflect.isExtensible(o) && Reflect.preventExtensions(o) && !Reflect.isExtensible(o) && !Reflect.defineProperty(o,'b',{value:1}) && !Reflect.set(o,'b',2) && !Reflect.setPrototypeOf(o,{}) && Reflect.setPrototypeOf(o,p)",
    );
    check(
        "let o={}; Reflect.defineProperty(o,'a',{value:4}) && Reflect.getOwnPropertyDescriptor(o,'a').value === 4 && !Reflect.defineProperty(o,'a',{value:5}) && !Reflect.deleteProperty(o,'a') && !Reflect.set(o,'a',5) && Reflect.deleteProperty(o,'missing') && Reflect.getOwnPropertyDescriptor(o,'missing') === undefined",
    );
    check(
        "let o={}, p={}; Reflect.setPrototypeOf(o,p) && !Reflect.setPrototypeOf(p,o) && Object.getPrototypeOf(p) === Object.prototype && Reflect.setPrototypeOf(o,null) && Reflect.getPrototypeOf(o) === null",
    );
}
#[test]
fn receivers_and_resumable_user_code() {
    check(
        "let o={get x(){return this.n+1;},set y(v){this.n=v;}}, r={n:4}; Reflect.get(o,'x',r) === 5 && Reflect.set(o,'y',7,r) && r.n === 7 && Reflect.set(o,'z',8,r) && r.z === 8 && !Reflect.has(o,'z') && !Reflect.set(o,'z',9,1)",
    );
    check("let o={x:1}, r={set x(v){throw 'wrong setter';}}; !Reflect.set(o,'x',2,r) && o.x === 1");
    check(
        "let log=''; let key={toString(){log+='k';return 'x';}}, d={configurable:true,get value(){log+='v';return 9;},get writable(){log+='w';return true;}}; let o={}; Reflect.defineProperty(o,key,d) && log === 'kvw' && Reflect.get(o,key) === 9 && Reflect.set(o,key,4) && Reflect.deleteProperty(o,key)",
    );
    check(
        "let o={get x(){throw 23;},set y(v){throw 24;}}, n=0; try{Reflect.get(o,'x');}catch(e){if(e===23)n++;} try{Reflect.set(o,'y',1);}catch(e){if(e===24)n++;} n===2",
    );
}
#[test]
fn non_object_targets_and_bad_descriptors_throw() {
    check(
        "let n=0; for(let f of [Reflect.defineProperty,Reflect.deleteProperty,Reflect.get,Reflect.getOwnPropertyDescriptor,Reflect.getPrototypeOf,Reflect.has,Reflect.isExtensible,Reflect.ownKeys,Reflect.preventExtensions,Reflect.set,Reflect.setPrototypeOf]){try{f(1,'x',{});}catch(e){if(e instanceof TypeError)n++;}} n===11",
    );
    check(
        "let n=0; for(let d of [1,{get:2},{get(){},value:1}]){try{Reflect.defineProperty({},'x',d);}catch(e){if(e instanceof TypeError)n++;}} try{Reflect.setPrototypeOf({},1);}catch(e){if(e instanceof TypeError)n++;} n===4",
    );
}
#[test]
fn constructors_and_new_target() {
    check(
        "function F(a,b){this.sum=a+b;this.target=new.target;} function G(){} let o=Reflect.construct(F,[2,3],G); o.sum===5 && o.target===G && Object.getPrototypeOf(o)===G.prototype && !(o instanceof F)",
    );
    check(
        "function G(){} let a=Reflect.construct(Array,[1,2],G), m=Reflect.construct(Map,[],G); Array.isArray(a) && a.length===2 && a[1]===2 && Object.getPrototypeOf(a)===G.prototype && Object.getPrototypeOf(m)===G.prototype && Map.prototype.has.call(m,1)===false",
    );
    check(
        "function F(x){this.x=x;} let log=''; let list={get length(){log+='l';return 2;},get 0(){log+='a';return 4;},get 1(){log+='b';return 5;}}; let o=Reflect.construct(F,list); o.x===4 && log==='lab' && o instanceof F",
    );
    check(
        "let n=0; for(let f of [()=>1,Reflect.get,{},1]){try{Reflect.construct(f,[]);}catch(e){if(e instanceof TypeError)n++;}} try{Reflect.construct(Object,[],()=>1);}catch(e){if(e instanceof TypeError)n++;} try{Reflect.construct(Object,1);}catch(e){if(e instanceof TypeError)n++;} n===6",
    );
}
#[test]
fn proxy_internal_paths() {
    check(
        "let s=Symbol('s'), o={a:1}; o[s]=2; let log=''; let p=new Proxy(o,{ownKeys(t){log+='k';return ['a',s];},getOwnPropertyDescriptor(t,k){log+='d';return Object.getOwnPropertyDescriptor(t,k);}}); let keys=Reflect.ownKeys(p); keys.length===2 && keys[1]===s && Reflect.getOwnPropertyDescriptor(p,'a').value===1 && log==='kd'",
    );
    check(
        "let o={x:1}, p=new Proxy(o,{}); Reflect.get(p,'x')===1 && Reflect.set(p,'x',2) && o.x===2 && Reflect.has(p,'x') && Reflect.defineProperty(p,'y',{value:3,configurable:true}) && Reflect.deleteProperty(p,'y') && Reflect.getPrototypeOf(p)===Object.prototype && Reflect.isExtensible(p) && Reflect.preventExtensions(p) && !Reflect.isExtensible(p)",
    );
    check(
        "let n=0; for(let name of ['get','set','has','defineProperty','deleteProperty','getPrototypeOf','setPrototypeOf','isExtensible','preventExtensions']){let h={};h[name]=()=>true;let p=new Proxy({},h); try{Reflect[name](p,'x',{});}catch(e){if(e instanceof TypeError)n++;}} n===9",
    );
}

#[test]
fn method_metadata_and_symbol_keys() {
    check(
        "let s=Symbol('x'),n=0,key={[Symbol.toPrimitive](hint){n++;if(hint!=='string')throw 'hint';return s;}};let o={};Reflect.defineProperty(o,key,{value:3,writable:true,configurable:true}) && Reflect.get(o,key)===3 && Reflect.has(o,key) && Reflect.set(o,key,4) && Reflect.getOwnPropertyDescriptor(o,key).value===4 && Reflect.deleteProperty(o,key) && n===6",
    );
    check(
        "let ok=true; for(let name of Object.getOwnPropertyNames(Reflect)){let f=Reflect[name],d=Object.getOwnPropertyDescriptor(Reflect,name);let len=2;if(name==='apply'||name==='defineProperty'||name==='set')len=3;if(name==='getPrototypeOf'||name==='isExtensible'||name==='ownKeys'||name==='preventExtensions')len=1;ok=ok&&f.name===name&&f.length===len&&d.writable&&!d.enumerable&&d.configurable;} ok",
    );
    check(
        "let s=Symbol('x'), n=0, key={toString(){n++;return s;}}; let o={}; Reflect.defineProperty(o,key,{value:3,writable:true,configurable:true}) && Reflect.get(o,key)===3 && Reflect.has(o,key) && Reflect.set(o,key,4) && Reflect.getOwnPropertyDescriptor(o,key).value===4 && Reflect.deleteProperty(o,key) && n===6",
    );
    check(
        "let o={get x(){'use strict';return this;},set x(v){'use strict';if(this!==undefined)throw 'receiver';}}; Reflect.get(o,'x',undefined)===undefined && Reflect.set(o,'x',1,undefined)",
    );
}
#[test]
fn ordinary_and_builtin_new_target_accessors() {
    check(
        "let n=0,p={};function F(){this.target=new.target;}let G=F.bind(null);Object.defineProperty(G,'prototype',{get(){n++;return p;}});let o=Reflect.construct(F,[],G),a=Reflect.construct(Array,[1,2],G);n===2 && Object.getPrototypeOf(o)===p && o.target===G && Array.isArray(a) && a.length===2 && Object.getPrototypeOf(a)===p",
    );
    check(
        "function F(){return {override:true};}function G(){} G.prototype=1; let o=Reflect.construct(F,[],G);o.override && Object.getPrototypeOf(o)===Object.prototype",
    );
    check(
        "let B=Array.bind(null,1); let a=Reflect.construct(B,[2]); a.length===2 && a[0]===1 && a[1]===2 && a instanceof Array",
    );
    check(
        "class A {constructor(){this.target=new.target;}} class B extends A {} function G(){} let o=Reflect.construct(B,[],G);o.target===G && Object.getPrototypeOf(o)===G.prototype",
    );
}
#[test]
fn array_and_typed_array_conversion_and_rejection() {
    check(
        "let a=[1,2,3], n=0, v={valueOf(){n++;return 1;}}; Reflect.defineProperty(a,'length',{value:v}) && a.length===1 && n===2",
    );
    check(
        "let a=[1,2,3], n=0, v={valueOf(){n++;return 2;}}; Reflect.set(a,'length',v) && a.length===2 && n===2",
    );
    check(
        "let a=[1,2,3]; Object.defineProperty(a,'1',{configurable:false}); !Reflect.defineProperty(a,'length',{value:0}) && a.length===2 && !(2 in a) && !Reflect.set(a,'length',0) && a.length===2",
    );
    check(
        "let a=new Uint8Array(1), n=0, v={valueOf(){n++;return 258;}}; Reflect.defineProperty(a,'0',{value:v}) && a[0]===2 && n===1 && Reflect.set(a,'0',v) && a[0]===2 && n===2 && Reflect.set(a,'9',v) && n===3 && !Reflect.defineProperty(a,'9',{value:v}) && n===3",
    );
    check(
        "let n=0; for(let o of [[1],new Uint8Array(1)]){let key=Array.isArray(o)?'length':'0';try{Reflect.defineProperty(o,key,{value:Symbol()});}catch(e){if(e instanceof TypeError)n++;}} try{Reflect.defineProperty([],'length',{value:-1});}catch(e){if(e instanceof RangeError)n++;}n===3",
    );
    check(
        "let n=0;try{Reflect.set(new Uint8Array(1),'0',{valueOf(){throw 45;}});}catch(e){if(e===45)n++;} try{Reflect.defineProperty({},'x',{get value(){throw 46;}});}catch(e){if(e===46)n++;}n===2",
    );
}

#[test]
fn getters_wait_collect_and_migrate_without_replay() {
    for source in [
        "let n=0,o={get x(){n++;return __rt.need(5,3);}}; Reflect.get(o,'x')===7 && n===1",
        "let n=0,o={set x(v){n++;this.y=__rt.need(5,3)+v;}}; Reflect.set(o,'x',2) && o.y===9 && n===1",
        "let n=0,o={}; Reflect.defineProperty(o,'x',{get value(){n++;return __rt.need(5,3);}}) && o.x===7 && n===1",
        "let n=0;function F(x){this.x=x;}let o=Reflect.construct(F,{get length(){n++;return __rt.need(5,3)-6;},0:9});o.x===9 && n===1",
        "let n=0;function F(x){this.x=x;}let o=Reflect.construct(F,{length:1,get 0(){n++;return __rt.need(5,3);}});o.x===7 && n===1",
        "let n=0,p={};function F(){this.target=new.target;}let G=F.bind(null);Object.defineProperty(G,'prototype',{get(){n++;__rt.need(5,3);return p;}});let o=Reflect.construct(F,[],G);o.target===G && Object.getPrototypeOf(o)===p && n===1",
        "let n=0,o={a:1},p=new Proxy(o,{ownKeys(){n++;__rt.need(5,3);return ['a'];}});Reflect.ownKeys(p)[0]==='a' && n===1",
        "let n=0,p=new Proxy({},{getOwnPropertyDescriptor(){n++;return {value:__rt.need(5,3),configurable:true};}});Reflect.getOwnPropertyDescriptor(p,'x').value===7 && n===1",
        "let n=0,r=new Proxy({},{getOwnPropertyDescriptor(){n++;__rt.need(5,3);return undefined;}});Reflect.set({x:1},'x',9,r) && r.x===9 && n===1",
    ] {
        check_waiting(source);
    }
}
#[test]
fn bound_constructor_setup_on_small_stack() {
    let runtime = Runtime::new();
    runtime
        .spawn(&compile_script("warm", "Object").unwrap(), &[])
        .unwrap()
        .run()
        .unwrap();
    let task = runtime
        .spawn(
            &compile_script("setup", "let F=function(){}; let G=F.bind(null); true").unwrap(),
            &[],
        )
        .unwrap();
    assert!(matches!(
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || task.run().unwrap())
            .unwrap()
            .join()
            .unwrap(),
        RunOutcome::Complete(_)
    ));
}

#[test]
fn validation_precedes_callbacks_and_proxy_invariants_throw() {
    check(
        "let n=0,key={toString(){throw 'key ran';}};for(let f of [Reflect.get,Reflect.set,Reflect.has,Reflect.defineProperty,Reflect.deleteProperty,Reflect.getOwnPropertyDescriptor]){try{f(null,key,{});}catch(e){if(e instanceof TypeError)n++;}}n===6",
    );
    check(
        "let n=0,list={get length(){throw 'list ran';}};try{Reflect.construct(()=>{},list);}catch(e){if(e instanceof TypeError)n++;}try{Reflect.construct(Object,list,undefined);}catch(e){if(e instanceof TypeError)n++;}n===2",
    );
    check(
        "let n=0;for(let keys of [['x','x'],[]]){let o={};Object.defineProperty(o,'x',{value:1});let p=new Proxy(o,{ownKeys(){return keys;}});try{Reflect.ownKeys(p);}catch(e){if(e instanceof TypeError)n++;}}let p=new Proxy({},{getOwnPropertyDescriptor(){return 1;}});try{Reflect.getOwnPropertyDescriptor(p,'x');}catch(e){if(e instanceof TypeError)n++;}n===3",
    );
    check(
        "let n=0;for(let name of ['get','set','has','defineProperty','deleteProperty','getPrototypeOf','isExtensible','preventExtensions']){let h={};h[name]=()=>true;let p=new Proxy({},h);try{Reflect[name](p,'x',{});}catch(e){if(e instanceof TypeError && e.message.indexOf('unsupported Proxy trap: '+name)>=0)n++;}}let p=new Proxy({},{setPrototypeOf(){return true;}});try{Reflect.setPrototypeOf(p,{});}catch(e){if(e instanceof TypeError && e.message.indexOf('unsupported Proxy trap: setPrototypeOf')>=0)n++;}n===9",
    );
}

#[test]
fn rejection_results_do_not_depend_on_property_names() {
    check(
        "let ok=true;for(let key of ['immutable','built-in objects are shared by all tasks and immutable','functions shared between tasks are immutable']){let o={};Object.defineProperty(o,key,{value:1});ok=ok&&!Reflect.set(o,key,2)&&!Reflect.defineProperty(o,key,{value:2})&&o[key]===1;let p=Object.preventExtensions({});ok=ok&&!Reflect.defineProperty(p,key,{value:1})&&!Reflect.set(p,key,1);}ok",
    );
    check(
        "let n=0;for(let operation of [()=>Reflect.set(Object.prototype,'immutable',1),()=>Reflect.defineProperty(Object.prototype,'immutable',{value:1}),()=>Reflect.preventExtensions(Object.prototype),()=>Reflect.setPrototypeOf(Object.prototype,{})]){try{operation();}catch(e){if(e instanceof TypeError)n++;}}n===4",
    );
}

#[test]
fn typed_array_definition_detaches_during_conversion() {
    check(
        "let a=new Uint8Array(1),n=0;let ok=Reflect.defineProperty(a,'0',{value:{valueOf(){n++;a.buffer.transfer();return 7;}}});ok && n===1 && a.length===0",
    );
    check(
        "let a=new Uint8Array(1),n=0;let ok=Reflect.set({x:1},'0',{valueOf(){n++;a.buffer.transfer();return 7;}},a);ok && n===1 && a.length===0",
    );
    check(
        "let a=new Uint8Array(1);Reflect.defineProperty(a,'0',{}) && Reflect.defineProperty(a,'0',{writable:true}) && a[0]===0",
    );
}
#[test]
fn proxy_definition_keeps_resolved_target_during_coercion() {
    check(
        "let a=[1,2],h={},p=new Proxy(a,h),n=0;let v={valueOf(){n++;h.defineProperty=()=>true;return 1;}};Reflect.defineProperty(p,'length',{value:v}) && a.length===1 && n===2",
    );
    check(
        "let a=[1,2],h={},p=new Proxy(a,h),n=0;let v={valueOf(){n++;h.defineProperty=()=>true;return 1;}};Reflect.set({},'length',v,p) && a.length===1 && n===2",
    );
    check(
        "let a=new Uint8Array(1),h={},p=new Proxy(a,h),n=0;let v={valueOf(){n++;h.defineProperty=()=>true;return 7;}};Reflect.defineProperty(p,'0',{value:v}) && a[0]===7 && n===1",
    );
}

fn check_waiting(source: &str) {
    use big_boy_pants_js::{Completion, Primitive};
    let code = compile_script("reflect-wait.js", source).unwrap();
    let runtime = Runtime::new();
    // Initialize realm on the host before entering a 64 KiB worker stack.
    let warm = compile_script("warm.js", "Object").unwrap();
    runtime.spawn(&warm, &[]).unwrap().run().unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    let mut waited = false;
    for _ in 0..100_000 {
        let result = std::thread::scope(|s| {
            std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn_scoped(s, || task.run_with(Budget::instructions(5), &mut NoHost))
                .unwrap()
                .join()
                .unwrap()
        })
        .unwrap();
        match result {
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                assert!(!waited, "callback replayed: {source}");
                waited = true;
                runtime.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = runtime.root(Primitive::Number(7.0));
                let wakeups = runtime
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap();
                assert_eq!(wakeups.into_runnable().len(), 1);
            }
            RunOutcome::Complete(v) => {
                assert!(waited, "did not wait: {source}");
                assert_eq!(v.view(), ValueView::Bool(true), "{source}");
                break;
            }
            other => panic!("{source}: {other:?}"),
        }
    }
    assert!(
        matches!(task.status(), big_boy_pants_js::TaskStatus::Finished),
        "instruction bound: {source}"
    );
}

#[test]
fn numeric_conversions_wait_collect_and_migrate_without_replay() {
    for source in [
        "let n=0,a=new Uint8Array(1),v={[Symbol.toPrimitive](hint){if(hint!=='number')throw 'hint';n++;return __rt.need(5,3);}};Reflect.defineProperty(a,'0',{value:v}) && a[0]===7 && n===1",
        "let n=0,a=[1,2],v={[Symbol.toPrimitive](hint){if(hint!=='number')throw 'hint';n++;if(n===2)__rt.need(5,3);return 1;}};Reflect.defineProperty(a,'length',{value:v}) && a.length===1 && n===2",
        "let n=0,a=new Uint8Array(1),v={valueOf(){n++;return __rt.need(5,3);}};Reflect.set(a,'0',v) && a[0]===7 && n===1",
        "let n=0,a=new Uint8Array(1),v={valueOf(){n++;return __rt.need(5,3);}};Reflect.defineProperty(a,'0',{value:v}) && a[0]===7 && n===1",
        "let n=0,a=new Uint8Array(1),v={valueOf(){n++;let x=__rt.need(5,3);a.buffer.transfer();return x;}};Reflect.defineProperty(a,'0',{value:v}) && a.length===0 && n===1",
        "let n=0,a=new Uint8Array(1),v={valueOf(){n++;let x=__rt.need(5,3);a.buffer.transfer();return x;}};Reflect.set({},'0',v,a) && a.length===0 && n===1",
        "let n=0,a=[1,2],v={valueOf(){n++;if(n===1)__rt.need(5,3);return 1;}};Reflect.defineProperty(a,'length',{value:v}) && a.length===1 && n===2",
        "let n=0,a=[1,2],v={valueOf(){n++;if(n===2)__rt.need(5,3);return 1;}};Reflect.defineProperty(a,'length',{value:v}) && a.length===1 && n===2",
        "let n=0,a=[1,2],v={valueOf(){n++;if(n===1)__rt.need(5,3);return 1;}};Reflect.set(a,'length',v) && a.length===1 && n===2",
        "let n=0,a=[1,2],v={valueOf(){n++;if(n===2)__rt.need(5,3);return 1;}};Reflect.set(a,'length',v) && a.length===1 && n===2",
        "let n=0,a=[1,2],h={},p=new Proxy(a,h),v={valueOf(){n++;if(n===1)__rt.need(5,3);h.defineProperty=()=>true;return 1;}};Reflect.defineProperty(p,'length',{value:v}) && a.length===1 && n===2",
    ] {
        check_waiting(source);
    }
}
