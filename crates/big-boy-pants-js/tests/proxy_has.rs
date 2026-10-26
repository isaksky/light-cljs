use big_boy_pants_js::frontend::compile_script;
use big_boy_pants_js::{
    Budget, Completion, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
};

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected host effect {}", call.op))
    }
}

#[test]
fn has_traps_follow_ecmascript() {
    let code = compile_script("proxy-has", include_str!("fixtures/js/proxy_has.js")).unwrap();
    let rt = Runtime::new();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(matches!(result, RunOutcome::Complete(v) if v.view() == ValueView::Bool(true)));
}

#[test]
fn has_traps_wait_collect_and_migrate_without_replay() {
    let source = r#"
let log = '';
function pause(n) {log += n; __rt.need(950 + n, 1);}
let target = new Proxy({x: 1}, {
    get getOwnPropertyDescriptor() {pause(3); return function(t, k) {pause(4); return Object.getOwnPropertyDescriptor(t, k);};},
    get isExtensible() {pause(5); return function(t) {pause(6); return true;};}
});
let handler = {get has() {pause(1); return function(t, k) {pause(2); return false;};}};
let p = Object.create(new Proxy(target, handler));
!Reflect.has(p, {toString() {pause(0); return 'x';}}) && log === '0123456';
"#;
    let rt = Runtime::new();
    let code = compile_script("proxy-has-waits", source).unwrap();
    let mut task = rt.spawn(&code, &[]).unwrap();
    let mut waits = 0;
    let mut yields = 0;
    for _ in 0..2000 {
        let (next, outcome) = std::thread::spawn(move || {
            let outcome = task
                .run_with(Budget::instructions(5), &mut NoEffects)
                .unwrap();
            (task, outcome)
        })
        .join()
        .unwrap();
        task = next;
        match outcome {
            RunOutcome::Yielded => {
                yields += 1;
                rt.collect().unwrap();
            }
            RunOutcome::Waiting(_) => {
                waits += 1;
                assert!(waits <= 7);
                rt.collect().unwrap();
                let request = task.take_producer_requests().pop().unwrap();
                let value = rt.root(Primitive::Undefined);
                task = rt
                    .complete(request.key(), request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                assert_eq!(waits, 7);
                assert!(yields > 20);
                return;
            }
            other => panic!("{other:?}"),
        }
    }
    panic!("Proxy has callbacks exceeded instruction bound");
}

#[test]
fn builtin_presence_checks_resume_without_replaying_indices() {
    for body in [
        "let r=Array.prototype.map.call(p,x=>x*2); r.join(',')==='2,,6' && log==='012'",
        "Array.prototype.flatMap.call(p,x=>[x]).join(',')==='1,3' && log==='012'",
        "[1].flatMap(()=>p).join(',')==='1,3' && log==='012'",
        "[p].flat().join(',')==='1,3' && log==='012'",
        "Array.prototype.slice.call(p).join(',')==='1,,3' && log==='012'",
        "Array.prototype.indexOf.call(p,2)===-1 && log==='012'",
        "Array.prototype.sort.call(p); target.join(',')==='1,3,' && log==='012'",
        "Array.prototype.reverse.call(p); target.join(',')==='3,2,1' && log==='02'",
        "Array.prototype.shift.call(p)===1 && target.join(',')===',3' && log==='12'",
        "Array.prototype.unshift.call(p,0); target.join(',')==='0,1,,3' && log==='210'",
        "let r=Array.prototype.splice.call(p,0,2); r.join(',')==='1,' && target.join(',')==='3' && log==='012'",
        "Array.prototype.copyWithin.call(p,0,1); target.join(',')===',3,3' && log==='12'",
        "let d=new Proxy({value:42},{has:h}); Object.defineProperty({},'x',d).x===42 && log==='enumerableconfigurablevaluewritablegetset'",
        "let o=new Proxy({cause:42},{has:h}); new Error('x',o).cause===42 && log==='cause'",
        "let l=new Proxy({length:2,0:'en',1:'fr'},{has:h}); Intl.getCanonicalLocales(l).join(',')==='en' && log==='01'",
    ] {
        let source = format!(
            r#"
let log='';
function h(t,k) {{log+=k; __rt.need(980,1); return k!=='1' && k in t;}}
let target=[1,2,3],p=new Proxy(target,{{has:h}});
{body}
"#
        );
        let rt = Runtime::new();
        let code = compile_script("builtin-has-wait", &source).unwrap();
        let mut task = rt.spawn(&code, &[]).unwrap();
        let mut waits = 0;
        for steps in 0..2000 {
            match task
                .run_with(Budget::instructions(5), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    assert_eq!(waits, 1, "{body}");
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Undefined);
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{body}");
                    assert_eq!(waits, 1, "{body}");
                    break;
                }
                other => panic!("{body}: {other:?}"),
            }
            assert!(steps < 1999, "{body}: exceeded instruction bound");
        }
    }
}
