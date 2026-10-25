//! Synthetic fixtures, not light-cljs output.
use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Budget, RunOutcome, Runtime, ValueView};

struct NoEffects;
impl clj_quickjs::Host for NoEffects {
    fn effect(&mut self, _: clj_quickjs::HostCall) -> Result<clj_quickjs::Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn check(source: &str) {
    let runtime = Runtime::new();
    let code = compile_script("object-statics", source).unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    for _ in 0..30_000 {
        match task
            .run_with(Budget::instructions(1), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true), "{source}");
                return;
            }
            other => panic!("{other:?}: {source}"),
        }
    }
    panic!("instruction bound: {source}");
}

#[test]
fn object_statics_enumeration_and_assignment() {
    for source in [
        "Object.assign.length === 2 && Object.entries.length === 1 && Object.values.length === 1 && Object.fromEntries.length === 1 && Object.defineProperties.length === 2",
        "let s = Symbol(), o = {b:2, 2:20, 1:10, [s]:3}; Object.defineProperty(o,'hidden',{value:9}); Object.entries(o).map(e=>e.join(':')).join() === '1:10,2:20,b:2' && Object.values(o).join() === '10,20,2'",
        "let log = '', o = {get a(){log+='a'; delete this.b; this.c=3; return 1;}, b:2}; Object.values(o).join() === '1' && log === 'a'",
        "Object.entries('ab').map(e=>e.join(':')).join() === '0:a,1:b' && Object.values(7).length === 0",
        "let a=[]; a[3]=7; let e=Object.entries(a); e.length===1 && e[0][0]==='3' && e[0][1]===7",
        "let source={get a(){Object.defineProperty(this,'b',{enumerable:false}); this.c=3; return 1;},b:2}; let t=Object.assign({},source); t.a===1 && !('b' in t) && !('c' in t)",
        "let s=Symbol(), log='', p={set a(v){log+='s'+v; this.x=v;}}; let t=Object.create(p); let o={get a(){log+='g'; return 7;}, [s]:8}; Object.defineProperty(o,'hide',{value:4}); Object.assign(t,null,undefined,o,{z:9}) === t && log === 'gs7' && t.x === 7 && t[s] === 8 && t.z === 9 && !('hide' in t) && !Object.getOwnPropertyDescriptor(t,'a')",
        "let o={}; Object.defineProperty(o,'__proto__',{value:null,enumerable:true}); let t=Object.assign({},o); Object.getPrototypeOf(t) === null",
        "let t={}; Object.defineProperty(t,'b',{value:0}); let ok=false; try{Object.assign(t,{a:1,b:2,c:3});}catch(e){ok=e instanceof TypeError;} ok && t.a === 1 && !('c' in t)",
        "let n=Object.assign(7,{a:1}); typeof n === 'object' && n.valueOf() === 7 && n.a === 1",
        "let ok=false; try{Object.assign(Object,{x:1});}catch(e){ok=e instanceof TypeError;} ok",
        "let s=Symbol(), log=''; let p=new Proxy({a:1,[s]:2},{ownKeys(){log+='k'; return [s,'a'];},getOwnPropertyDescriptor(t,k){log+='d';return {enumerable:true,configurable:true};}}); Object.values(p).join() === '1' && log === 'kd'",
        "let log=''; let p=new Proxy({a:1},{ownKeys(){log+='k'; return ['a'];},getOwnPropertyDescriptor(t,k){log+='d';return {enumerable:true,configurable:true};}}); Object.assign({},p).a === 1 && log === 'kd'",
        "let n=0; for(let f of [Object.assign,Object.entries,Object.values,Object.defineProperties]){try{f(null);}catch(e){if(e instanceof TypeError)n++;}} n===4",
    ] {
        check(source);
    }
}

#[test]
fn object_statics_from_entries_and_iterator_close() {
    for source in [
        "let s=Symbol(), o=Object.fromEntries(new Set([['a',1],[s,2],['a',3],['__proto__',7]])); o.a===3 && o[s]===2 && o.__proto__===7 && Object.getPrototypeOf(o)===Object.prototype && Object.getOwnPropertyDescriptor(o,'a').writable",
        "let log='', key={toString(){log+='k';return 'a';}}; function* entries(){yield {get 0(){log+='0';return key;},get 1(){log+='1';return 7;}};} Object.fromEntries(entries()).a===7 && log==='01k'",
        "let closed=0, error={}, ok=false; let it={[Symbol.iterator](){return this;}, next(){return {value:[{toString(){throw error;}},2]};},return(){closed++;throw 9;}}; try{Object.fromEntries(it);}catch(e){ok=e===error;} ok && closed===1",
        "let closed=0, ok=false; function* it(){try{yield 1;}finally{closed++;}} try{Object.fromEntries(it());}catch(e){ok=e instanceof TypeError;} ok && closed===1",
        "let n=0; for(let x of [null,undefined,1,'ab']){try{Object.fromEntries(x);}catch(e){if(e instanceof TypeError)n++;}} n===4",
        "let closed=0, error={}, ok=false; let it={[Symbol.iterator](){return this;},next(){throw error;},return(){closed++;return {};}}; try{Object.fromEntries(it);}catch(e){ok=e===error;} ok && closed===0",
        "let s=Symbol(), k={[Symbol.toPrimitive](hint){if(hint!=='string')throw 9;return s;}}; let o=Object.fromEntries([[k,1],[12n,2],[-0,3],['12',4]]); o[s]===1 && o[12]===4 && o[0]===3",
        "let closed=0, error={}, ok=false; let it={[Symbol.iterator](){return this;},next(){return {get value(){throw error;}};},return(){closed++;return {};}}; try{Object.fromEntries(it);}catch(e){ok=e===error;} ok && closed===0",
    ] {
        check(source);
    }
}

#[test]
fn object_statics_define_properties_conversion_before_writes() {
    for source in [
        "let s=Symbol(), t={}, d={a:{value:7,enumerable:true},[s]:{get(){return 8;}}}; Object.defineProperty(d,'hide',{value:{value:9}}); Object.defineProperties(t,d)===t && t.a===7 && t[s]===8 && !('hide' in t) && !Object.getOwnPropertyDescriptor(t,'a').writable",
        "let t={}, ok=false; try{Object.defineProperties(t,{a:{value:1},b:{get:7}});}catch(e){ok=e instanceof TypeError;} ok && !('a' in t)",
        "let log='', t={}; let d={get a(){log+='a';return {get value(){log+='v';return 7;}};},get b(){log+=('a' in t)?'!':'b';return {value:8};}}; Object.defineProperties(t,d)===t && t.a===7 && t.b===8 && log==='avb'",
        "let t={}; Object.defineProperty(t,'b',{value:0}); let ok=false; try{Object.defineProperties(t,{a:{value:1},b:{value:2},c:{value:3}});}catch(e){ok=e instanceof TypeError;} ok && t.a===1 && !('c' in t)",
        "let log='', p=new Proxy({a:{value:7}},{ownKeys(){log+='k';return ['a'];},getOwnPropertyDescriptor(){log+='d';return {enumerable:true,configurable:true};}}); let t=Object.defineProperties({},p); t.a===7 && log==='kd'",
        "let t=new Int32Array(1); Object.defineProperties(t,{'0':{value:{valueOf(){return 7;}}}})===t && t[0]===7",
        "let n=0; for(let p of [null,undefined]){try{Object.defineProperties({},p);}catch(e){if(e instanceof TypeError)n++;}} n===2 && Object.keys(Object.defineProperties({},7)).length===0",
        "let d={value:1}, t={}; Object.defineProperties(t,{a:d,get b(){d.value=2;return {value:3};}}); t.a===1 && t.b===3",
        "let p={set a(v){throw 99;}}; let t=Object.create(p); Object.defineProperties(t,{a:{value:7}}); t.a===7",
    ] {
        check(source);
    }
}

#[test]
fn object_statics_on_small_worker_stack() {
    let runtime = Runtime::new();
    runtime
        .spawn(&compile_script("warm", "Object").unwrap(), &[])
        .unwrap()
        .run()
        .unwrap();
    let code = compile_script("small-stack", "let t=Object.assign({}, {get a(){return 7;}}); let o=Object.fromEntries(Object.entries(t)); Object.defineProperties(o,{b:{get(){return 8;},enumerable:true}}); let v=Object.values(o); v[0]===7 && v[1]===8").unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || match task.run().unwrap() {
            RunOutcome::Complete(value) => value.view(),
            other => panic!("{other:?}"),
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(result, ValueView::Bool(true));
}

#[test]
fn object_statics_results_are_task_owned() {
    for source in [
        "Object.entries({a:1})",
        "Object.values({a:1})",
        "Object.fromEntries([['a',1]])",
        "Object.assign({}, {a:1})",
        "Object.defineProperties({}, {a:{value:1}})",
    ] {
        let runtime = Runtime::new();
        let code = compile_script("ownership", source).unwrap();
        let RunOutcome::Complete(value) = runtime.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("{source}");
        };
        assert!(
            matches!(
                runtime.publish(&value),
                Err(clj_quickjs::PublishError::NotShareable { .. })
            ),
            "{source}"
        );
    }
}
