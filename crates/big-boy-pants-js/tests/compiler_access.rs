//! Compiler call preparation/root reads use the ordinary saved property
//! continuations. These synthetic engine tests exercise waits and GC directly.
use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    Budget, Completion, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
};

#[derive(Default)]
struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        Err("unexpected host effect".into())
    }
}

fn run(source: &str, waits: usize) {
    let rt = Runtime::new();
    // Compiler workers enter an initialized realm. Build that realm before
    // moving the selected call/root path onto its 64 KiB execution stack.
    let warm = compile_function("function warm(){return [];}").unwrap();
    assert!(matches!(
        rt.spawn(&warm, &[]).unwrap().run().unwrap(),
        RunOutcome::Complete(_)
    ));
    let code = compile_function(source).unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut waited = 0;
            for _ in 0..100_000 {
                match task
                    .run_with(Budget::instructions(1), &mut NoEffects)
                    .unwrap()
                {
                    RunOutcome::Yielded => {
                        rt.collect().unwrap();
                    }
                    RunOutcome::Waiting(_) => {
                        waited += 1;
                        rt.collect().unwrap();
                        let request = task.take_producer_requests().pop().unwrap();
                        let ready = rt.root(Primitive::Null);
                        task = rt
                            .complete(request.key(), request.producer(), Completion::Ready(&ready))
                            .unwrap()
                            .into_runnable()
                            .pop()
                            .unwrap();
                    }
                    RunOutcome::Complete(value) => {
                        assert_eq!(value.view(), ValueView::Bool(true));
                        assert_eq!(waited, waits);
                        return;
                    }
                    other => panic!("{other:?}"),
                }
            }
            panic!("did not finish");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn call_members_keep_early_reads_receivers_and_operands_across_waits() {
    run(
        r#"function f() {
      const log=[];
      const fn=function(x,y){'use strict';if(this!==null)throw new Error('receiver');return x+y;};
      Object.defineProperty(fn,'call',{configurable:true,get(){log.push('get');__rt.need(90,1);return Function.prototype.call;}});
      const original=fn;
      if(__rt.cljCallable(original))throw new Error('function classified as collection');
      const method=__rt.cljCallMember(original,null);
      log.push('arg');__rt.need(91,1);
      Object.defineProperty(fn,'call',{value:()=>99});
      if(__rt.call(__rt.formValue(original),method,null,20,22)!==42)throw new Error('saved method');
      const object={offset:2,get call(){__rt.need(92,1);return function(_,x){return this.offset+x;};}};
      const member=__rt.cljCallMember(object,null);
      if(__rt.call(object,member,null,40)!==42)throw new Error('object receiver');
      let operands=0;
      try{const member=__rt.cljCallMember(null,null);operands++;}catch(e){if(!(e instanceof TypeError))throw e;}
      if(operands!==0||log.join(',')!=='get,arg')throw new Error('ordering');
      for(const v of [__rt.formVector(1),__rt.formMap(),__rt.formSet(),__rt.formKeyword(null,'k'),__rt.formSymbol(null,'s')])
        if(!__rt.cljCallable(v)||(__rt.formKind(v)!=='keyword'&&!__rt.cljCallable(__rt.cljWithMeta(v,__rt.formMap('tag',true)))))throw new Error('callable form');
      const T=__rt.structType('cljs.core/MultiFn',__rt.formVector(),false, null), m=__rt.structNew(T);
      const entry=function(_,x){return this===m?x:0;};
      if(__rt.cljCallMember(m,entry)!==entry)throw new Error('multimethod entry');
      return true;
    }"#,
        3,
    );
}

#[test]
fn proxy_call_members_keep_early_reads_across_waits() {
    run(include_str!("fixtures/js/compiler_proxy_calls.js"), 6);
}

#[test]
fn unbound_root_reads_check_invariants_after_getter_mutation() {
    run(include_str!("fixtures/js/compiler_root_invariants.js"), 12);
}

#[test]
fn root_reads_preserve_overlays_replacements_getters_and_registry_replacement() {
    run(
        r#"function f() {
      const target={}, overlays=new Map(), readers=new Map();
      const proxy=new Proxy(target,{get:(t,k,r)=>overlays.has(k)?overlays.get(k):Reflect.get(t,k,r)});
      const context=__rt.vector(proxy,target,overlays,readers);
      let calls=0;
      Object.defineProperty(target,'x',{configurable:true,get(){calls++;if(this!==proxy)throw new Error('root receiver');__rt.need(90,1);return 7;}});
      overlays.set('x',undefined);
      if(__rt.cljReadRoot(proxy,context,'x')!==undefined||calls!==0)throw new Error('undefined overlay');
      for(const v of [null,false,0]){overlays.set('x',v);if(__rt.cljReadRoot(proxy,context,'x')!==v)throw new Error('overlay');}
      overlays.delete('x');
      if(__rt.cljReadRoot(proxy,context,'x')!==7||calls!==1)throw new Error('getter');
      Object.defineProperty(target,'x',{configurable:true,value:8});
      if(__rt.cljReadRoot(proxy,context,'x')!==8)throw new Error('replacement');
      Object.defineProperty(target,'fixed',{value:7,writable:false,configurable:false});
      Object.defineProperty(target,'noGetter',{get:undefined,configurable:false});
      for(const key of ['fixed','noGetter']) {
        overlays.set(key,8);let failed=false;
        try{__rt.cljReadRoot(proxy,context,key);}catch(e){failed=e instanceof TypeError;}
        if(!failed)throw new Error('binding bypassed Proxy invariant');
      }
      overlays.set('fixed',7);overlays.set('noGetter',undefined);
      if(__rt.cljReadRoot(proxy,context,'fixed')!==7||__rt.cljReadRoot(proxy,context,'noGetter')!==undefined)
        throw new Error('valid fixed-property overlay');
      const other=new Proxy({x:9},{get(t,k,r){calls++;__rt.need(91,1);return Reflect.get(t,k,r);}});
      if(__rt.cljReadRoot(other,context,'x')!==9||calls!==2)throw new Error('registry replacement');
      return true;
    }"#,
        2,
    );
}

#[test]
fn ready_product_reads_guard_the_live_getter_and_keep_its_captured_name() {
    run(
        r#"function f() {
      const target={}, overlays=new Map(), readers=new Map();
      const proxy=new Proxy(target,{get:(t,k,r)=>Reflect.get(t,k,r)});
      const context=__rt.vector(proxy,target,overlays,readers);
      const bundle={state:0,products:{x:7}};let forces=0;
      const getter=()=>{forces++;__rt.need(90,1);bundle.state=2;return bundle.products.x;};
      readers.set(getter,__rt.vector(bundle,'x'));
      Object.defineProperty(target,'x',{configurable:true,get:getter});
      if(__rt.cljReadRoot(proxy,context,'x')!==7||forces!==1)throw new Error('pending force');
      bundle.products.x=8;
      Object.defineProperty(target,'alias',{get:getter});
      if(__rt.cljReadRoot(proxy,context,'alias')!==8||forces!==1)throw new Error('captured product name');
      Object.defineProperty(bundle.products,'x',{get(){if(this!==bundle.products)throw new Error('product receiver');__rt.need(91,1);return 9;}});
      if(__rt.cljReadRoot(proxy,context,'x')!==9||forces!==2)throw new Error('deferred product uses root getter');
      Object.defineProperty(target,'x',{get(){forces++;return 10;}});
      if(__rt.cljReadRoot(proxy,context,'x')!==10||forces!==3)throw new Error('replaced getter');
      return true;
    }"#,
        2,
    );
}

#[test]
fn definition_groups_and_missing_compiler_roots_keep_their_private_readers() {
    run(
        r#"function f() {
      const target={}, overlays=new Map(), readers=new Map();
      const proxy=new Proxy(target,{get:(t,k,r)=>Reflect.get(t,k,r)});
      const missing=new Proxy(target,{get(t,k){
        if(k==='cold'){__rt.need(94,1);Object.defineProperty(t,k,{value:42});return 42;}
        throw new Error('unavailable definition: '+k);
      }});
      const context=__rt.vector(proxy,target,overlays,readers,missing);
      const bundle={state:2,group:true,products:{}};let calls=0;
      const getter=()=>{calls++;__rt.need(93,1);return calls===1?7:9;};
      readers.set(getter,__rt.vector(bundle,'x'));
      Object.defineProperty(target,'x',{get:getter});
      if(__rt.cljReadRoot(proxy,context,'x')!==7)throw new Error('conditional previous generation');
      bundle.products.x=8;
      if(__rt.cljReadRoot(proxy,context,'x')!==9)throw new Error('executing owner view');
      if(__rt.cljReadRoot(proxy,context,'cold')!==42)throw new Error('lazy registration');
      if(proxy.unknown!==undefined)throw new Error('public property changed');
      let failed=false;
      try{__rt.cljReadRoot(proxy,context,'unknown');}catch(e){failed=e.message==='unavailable definition: unknown';}
      if(!failed)throw new Error('missing compiler root');
      return true;
    }"#,
        2,
    );
}
