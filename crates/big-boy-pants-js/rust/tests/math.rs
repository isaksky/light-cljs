//! Synthetic JavaScript fixtures, not light-cljs output (clj-quickjs-319).
use big_boy_pants_js::{
    Budget, Completion, Host, HostCall, Primitive, RunOutcome, Runtime, ValueView,
    frontend::compile_script,
    module::{ModuleHost, ModuleKey, ModuleLoader, ModuleSource},
};

struct NoEffects;
impl Host for NoEffects {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        Err("unexpected effect".into())
    }
}

fn check(source: &str) {
    let rt = Runtime::new();
    let code = compile_script("math.js", source).unwrap();
    let result = rt.spawn(&code, &[]).unwrap().run().unwrap();
    assert!(
        matches!(result, RunOutcome::Complete(ref v) if v.view() == ValueView::Bool(true)),
        "{source}: {result:?}"
    );
}

const UNARY: &[&str] = &[
    "round", "trunc", "sign", "log", "log10", "log1p", "exp", "expm1", "sin", "cos", "tan", "asin",
    "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "cbrt", "clz32", "f16round",
];

#[test]
fn remaining_math_members_and_coercions() {
    for &name in UNARY.iter().chain(["atan2", "min", "max", "random"].iter()) {
        let length = match name {
            "atan2" | "min" | "max" => 2,
            "random" => 0,
            _ => 1,
        };
        check(&format!(
            "let f = Math.{name}, d = Object.getOwnPropertyDescriptor(Math, '{name}'); typeof f === 'function' && d.value === f && d.writable && !d.enumerable && d.configurable && f.name === '{name}' && f.length === {length} && Object.getPrototypeOf(f) === Function.prototype && !('prototype' in f)"
        ));
        check(&format!(
            "let caught = false; try {{ new Math.{name}(); }} catch(e) {{ caught = e instanceof TypeError; }} caught"
        ));
    }
    for name in UNARY {
        check(&format!(
            "let log = ''; let o = {{get valueOf() {{ log += 'g'; return function() {{ log += 'v'; return {{}}; }}; }}, toString() {{ log += 's'; return '0.25'; }} }}; Object.is(Math.{name}(o), Math.{name}(0.25)) && log === 'gvs'"
        ));
        check(&format!(
            "let n = 0; let o = {{valueOf() {{ n++; throw 42; }} }}; let caught = false; try {{ Math.{name}(o); }} catch(e) {{ caught = e === 42; }} caught && n === 1 && Object.is(Math.{name}.call(null, 0.25, o), Math.{name}(0.25))"
        ));
        for value in ["1n", "Symbol()", "Object.create(null)"] {
            check(&format!(
                "let caught = false; try {{ Math.{name}({value}); }} catch(e) {{ caught = e instanceof TypeError; }} caught"
            ));
        }
        if *name == "clz32" {
            check("Math.clz32() === 32");
        } else {
            check(&format!("Number.isNaN(Math.{name}())"));
        }
    }
    check(
        "let log = ''; let a = {valueOf() { log += 'a'; return NaN; }}, b = {valueOf() { log += 'b'; return 0; }}; Number.isNaN(Math.atan2(a,b)) && log === 'ab'",
    );
    check(
        "Number.isNaN(Math.atan2()) && Number.isNaN(Math.atan2(1)) && Math.atan2('0', null) === 0",
    );
    for name in ["atan2", "min", "max"] {
        check(&format!(
            "let log = ''; let b = {{valueOf() {{log += 'b'; return 0;}}}}; let caught = false; try {{ Math.{name}(1n,b); }} catch(e) {{caught = e instanceof TypeError;}} caught && log === ''"
        ));
        check(&format!(
            "let log = ''; function v(n) {{return {{valueOf() {{log += n; return n;}}}};}} Object.is(Math.{name}(v(1),v(2)), Math.{name}(1,2)) && log === '12'"
        ));
    }
}

#[test]
fn remaining_math_edges() {
    for source in [
        "Math.min() === Infinity && Math.max() === -Infinity",
        "Object.is(Math.min(0,-0),-0) && Object.is(Math.min(-0,0),-0) && Object.is(Math.max(0,-0),0) && Object.is(Math.max(-0,0),0)",
        "Math.min(3,2,-7,Infinity) === -7 && Math.max(3,2,-7,-Infinity) === 3 && Number.isNaN(Math.min(NaN,Infinity)) && Number.isNaN(Math.max(-Infinity,NaN))",
        "let n = 0; let o = {valueOf() { n++; return 7; }}; Number.isNaN(Math.min(NaN,o)) && Number.isNaN(Math.max(NaN,o)) && n === 2",
        "let caught = false; try { Math.min(NaN, {valueOf() {throw 42;}}); } catch(e) {caught = e === 42;} caught",
        "Math.round(0.5) === 1 && Object.is(Math.round(-0.5),-0) && Math.round(-1.5) === -1 && Math.round(1.5) === 2",
        "Object.is(Math.round(-0),-0) && Object.is(Math.round(-Number.MIN_VALUE),-0) && Math.round(0.49999999999999994) === 0 && Math.round(4503599627370495.5) === 4503599627370496",
        "Math.round(Infinity) === Infinity && Math.round(-Infinity) === -Infinity && Math.round(Number.MAX_VALUE) === Number.MAX_VALUE",
        "Object.is(Math.trunc(-0.9),-0) && Math.trunc(-1.9) === -1 && Math.trunc(Infinity) === Infinity",
        "Object.is(Math.sign(-0),-0) && Object.is(Math.sign(0),0) && Math.sign(-Infinity) === -1 && Math.sign(Infinity) === 1 && Number.isNaN(Math.sign(NaN))",
        "Math.clz32(0) === 32 && Math.clz32(-1) === 0 && Math.clz32(1) === 31 && Math.clz32(4294967297.9) === 31 && Math.clz32(Infinity) === 32 && Math.clz32(NaN) === 32",
        "Math.f16round(1 + 2**-11) === 1 && Math.f16round(1 + 3*2**-11) === 1 + 2**-9 && Math.f16round(1 + 2**-11 + 2**-52) === 1 + 2**-10",
        "Math.f16round(65504) === 65504 && Math.f16round(65520) === Infinity && Math.f16round(-65520) === -Infinity && Math.f16round(2**-24) === 2**-24",
        "Object.is(Math.f16round(-(2**-25)),-0) && Math.f16round(2**-25) === 0 && Math.f16round(2**-25 + 2**-76) === 2**-24 && Object.is(Math.f16round(-0),-0)",
        "Math.log(0) === -Infinity && Math.log(-0) === -Infinity && Number.isNaN(Math.log(-1)) && Math.log(Infinity) === Infinity",
        "Math.log10(0) === -Infinity && Number.isNaN(Math.log10(-1)) && Math.log10(Infinity) === Infinity",
        "Object.is(Math.log1p(-0),-0) && Math.log1p(-1) === -Infinity && Number.isNaN(Math.log1p(-2)) && Math.log1p(Infinity) === Infinity",
        "Math.exp(-Infinity) === 0 && Math.exp(Infinity) === Infinity && Math.expm1(-Infinity) === -1 && Object.is(Math.expm1(-0),-0)",
        "Object.is(Math.sin(-0),-0) && Object.is(Math.tan(-0),-0) && Math.cos(-0) === 1 && Number.isNaN(Math.sin(Infinity)) && Number.isNaN(Math.cos(Infinity)) && Number.isNaN(Math.tan(Infinity))",
        "Object.is(Math.asin(-0),-0) && Object.is(Math.atan(-0),-0) && Number.isNaN(Math.asin(2)) && Number.isNaN(Math.acos(2)) && Math.acos(1) === 0 && Math.atan(Infinity) === Math.PI/2",
        "Object.is(Math.atan2(-0,0),-0) && Math.atan2(0,-0) === Math.PI && Math.atan2(-0,-0) === -Math.PI && Math.atan2(Infinity,Infinity) === Math.PI/4 && Math.atan2(-Infinity,Infinity) === -Math.PI/4",
        "Object.is(Math.sinh(-0),-0) && Object.is(Math.tanh(-0),-0) && Math.sinh(-Infinity) === -Infinity && Math.cosh(-Infinity) === Infinity && Math.cosh(-0) === 1 && Math.tanh(-Infinity) === -1",
        "Object.is(Math.asinh(-0),-0) && Math.asinh(-Infinity) === -Infinity && Math.acosh(1) === 0 && Number.isNaN(Math.acosh(0)) && Math.acosh(Infinity) === Infinity",
        "Object.is(Math.atanh(-0),-0) && Math.atanh(1) === Infinity && Math.atanh(-1) === -Infinity && Number.isNaN(Math.atanh(2))",
        "Object.is(Math.cbrt(-0),-0) && Math.cbrt(-Infinity) === -Infinity && Math.cbrt(Infinity) === Infinity",
    ] {
        check(source);
    }
}

#[test]
fn math_constants() {
    for (name, value) in [
        ("E", "2.718281828459045"),
        ("LN2", "0.6931471805599453"),
        ("LN10", "2.302585092994046"),
        ("LOG2E", "1.4426950408889634"),
        ("LOG10E", "0.4342944819032518"),
        ("PI", "3.141592653589793"),
        ("SQRT1_2", "0.7071067811865476"),
        ("SQRT2", "1.4142135623730951"),
    ] {
        check(&format!(
            "let d = Object.getOwnPropertyDescriptor(Math,'{name}'); d.value === {value} && !d.writable && !d.enumerable && !d.configurable"
        ));
    }
    check(
        "let d = Object.getOwnPropertyDescriptor(Math,Symbol.toStringTag); d.value === 'Math' && !d.writable && !d.enumerable && d.configurable && Object.prototype.toString.call(Math) === '[object Math]' && Object.keys(Math).length === 0",
    );
}

#[test]
fn remaining_math_waits_collects_and_migrates() {
    for expression in [
        "Math.sin(v(0,319)) === 0",
        "Math.f16round(v(1.5,319)) === 1.5",
        "Math.clz32(v(1,319)) === 31",
        "Math.atan2(v(0,319),v(1,320)) === 0",
        "Number.isNaN(Math.min(NaN,v(1,319),v(2,320)))",
        "Number.isNaN(Math.max(NaN,v(1,319),v(2,320)))",
        "Math.min(-7,v(2,319)) === -7",
        "Math.max(7,v(2,319)) === 7",
        "Math.max(v(1,319),v(2,320)) === 2",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "let n = 0; function v(x,id) {{ return {{get valueOf() {{ n++; __rt.need(id,1); return () => x; }} }}; }} let ok = {expression}; ok && n === {}",
            if expression.contains("320") { 2 } else { 1 }
        );
        let code = compile_script("math-wait.js", &source).unwrap();
        let mut task = rt.spawn(&code, &[]).unwrap();
        let mut done = false;
        let mut waits = 0;
        for _ in 0..1000 {
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
                RunOutcome::Waiting(_) => {
                    waits += 1;
                    rt.collect().unwrap();
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = rt.root(Primitive::Number(0.0));
                    task = rt
                        .complete(request.key(), request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Bool(true), "{expression}");
                    done = true;
                    break;
                }
                other => panic!("{expression}: {other:?}"),
            }
        }
        assert!(done, "{expression}");
        assert_eq!(waits, if expression.contains("320") { 2 } else { 1 });
    }
}

#[test]
fn math_random_is_task_local_and_deterministic() {
    let rt = Runtime::new();
    let code = compile_script("math-random.js", "let out = []; for(let i=0; i<16; i++) { let x = Math.random({valueOf() {throw 42;}}); if (!(x >= 0 && x < 1)) throw 43; out.push(x); } out.join(',')").unwrap();
    let mut results = Vec::new();
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        let mut task = rt.spawn(&code, &[]).unwrap();
        for _ in 0..2000 {
            let (next, result) = std::thread::spawn(move || {
                let result = task.run_with(budget, &mut NoEffects).unwrap();
                (task, result)
            })
            .join()
            .unwrap();
            task = next;
            match result {
                RunOutcome::Complete(value) => {
                    results.push(value.string().unwrap().to_string());
                    break;
                }
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], results[1]);
    let text = &results[0];
    let mut values: Vec<_> = text.split(',').collect();
    values.sort_unstable();
    values.dedup();
    assert_eq!(values.len(), 16);
}

#[test]
fn math_random_stream_survives_activation_replacement() {
    #[derive(Default)]
    struct Effects(Vec<Primitive>);
    impl Host for Effects {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.0.push(call.arg);
            Ok(Primitive::Undefined)
        }
    }

    fn run(rt: &Runtime, mut task: big_boy_pants_js::Task, budget: Budget) -> Vec<Primitive> {
        let mut host = Effects::default();
        for _ in 0..2000 {
            let (next, effects, result) = std::thread::spawn(move || {
                let result = task.run_with(budget, &mut host).unwrap();
                (task, host, result)
            })
            .join()
            .unwrap();
            task = next;
            host = effects;
            match result {
                RunOutcome::Complete(_) => return host.0,
                RunOutcome::Yielded => {
                    rt.collect().unwrap();
                }
                other => panic!("{other:?}"),
            }
        }
        panic!("random stream task did not complete");
    }

    let rt = Runtime::new();
    let regression = compile_script(
        "random-reaction.js",
        "let a=Math.random(); Promise.resolve().then(()=>__rt.effect(0,Math.random()===a));",
    )
    .unwrap();
    assert_eq!(
        run(&rt, rt.spawn(&regression, &[]).unwrap(), Budget::UNLIMITED),
        [Primitive::Bool(false)]
    );

    let reference = compile_script(
        "random-reference.js",
        "for(let i=0; i<6; i++) __rt.effect(0, Math.random());",
    )
    .unwrap();
    let expected = run(&rt, rt.spawn(&reference, &[]).unwrap(), Budget::UNLIMITED);
    assert_eq!(expected.len(), 6);

    const JOBS: &str = "Promise.resolve().then(() => __rt.effect(0, Math.random()));
        Promise.resolve().then((function() { __rt.effect(0, Math.random()); }).bind(null));";
    let script = compile_script(
        "random-jobs.js",
        &format!("for(let i=0; i<4; i++) __rt.effect(0, Math.random()); {JOBS}"),
    )
    .unwrap();

    struct Modules;
    impl ModuleHost for Modules {
        fn resolve(
            &mut self,
            specifier: &big_boy_pants_js::JsString,
            _: Option<ModuleKey>,
        ) -> Result<ModuleKey, String> {
            match specifier.to_string_lossy().as_str() {
                "entry.js" => Ok(ModuleKey::new(0)),
                "dep.js" => Ok(ModuleKey::new(1)),
                _ => Err("unknown module".into()),
            }
        }

        fn fetch(&mut self, key: ModuleKey) -> Result<ModuleSource, String> {
            let draws = "__rt.effect(0, Math.random()); __rt.effect(0, Math.random());";
            if key == ModuleKey::new(0) {
                Ok(ModuleSource::from_name(
                    "entry.js".into(),
                    format!("import 'dep.js'; {draws} {JOBS}"),
                ))
            } else if key == ModuleKey::new(1) {
                Ok(ModuleSource::from_name("dep.js".into(), draws.into()))
            } else {
                Err("unknown module".into())
            }
        }
    }
    let graph = ModuleLoader::new()
        .load(rt.world(), &"entry.js".into(), &mut Modules)
        .unwrap();
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        assert_eq!(
            run(&rt, rt.spawn(&script, &[]).unwrap(), budget),
            expected,
            "Promise jobs, {budget:?}"
        );
        assert_eq!(
            run(&rt, rt.spawn_module(&graph), budget),
            expected,
            "modules and Promise jobs, {budget:?}"
        );
    }
}

/// Exact binary64 pins from the system libm calls used by QuickJS's
/// js_math_funcs (revision 7d1c13c5d8f8), measured on Linux x86_64/glibc.
#[test]
#[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
fn math_libm_pinned_bits() {
    for (expression, bits) in [
        ("sin(1)", 0x3feaed548f090cee),
        ("sin(1e300)", 0xbfea2c16b010e385),
        ("cos(1)", 0x3fe14a280fb5068c),
        ("cos(1e300)", 0xbfe2699022adc4c1),
        ("tan(1)", 0x3ff8eb245cbee3a6),
        ("tan(1e300)", 0x3ff6be411f37ac77),
        ("exp(1)", 0x4005bf0a8b145769),
        ("exp(0.1)", 0x3ff1aec7b35a00d4),
        ("log(2)", 0x3fe62e42fefa39ef),
        ("log(0.1)", 0xc0026bb1bbb55515),
        ("log10(2)", 0x3fd34413509f79ff),
        ("log2(10)", 0x400a934f0979a371),
        ("cbrt(2)", 0x3ff428a2f98d728c),
        ("cbrt(-8)", 0xc000000000000000),
        ("expm1(1e-16)", 0x3c9cd2b297d889bc),
        ("log1p(1e-16)", 0x3c9cd2b297d889bc),
        ("sinh(1)", 0x3ff2cd9fc44eb982),
        ("cosh(1)", 0x3ff8b07551d9f550),
        ("tanh(1)", 0x3fe85efab514f394),
        ("asinh(1)", 0x3fec34366179d427),
        ("asinh(2)", 0x3ff719218313d087),
        ("asinh(1e308)", 0x40862f1d6695e8ec),
        ("acosh(2)", 0x3ff5124271980434),
        ("atanh(0.5)", 0x3fe193ea7aad030a),
        ("atanh(-0.5)", 0xbfe193ea7aad030a),
        ("atanh(-0.2)", 0xbfc9f323ecbf984c),
        ("atanh(-0.9)", 0xbff78e360604b32d),
        ("atanh(0.9)", 0x3ff78e360604b32d),
        ("acosh(1e308)", 0x40862f1d6695e8ec),
        ("asin(0.5)", 0x3fe0c152382d7366),
        ("acos(0.5)", 0x3ff0c152382d7366),
        ("atan(1)", 0x3fe921fb54442d18),
        ("atan2(1,2)", 0x3fddac670561bb4f),
        ("pow(2,0.5)", 0x3ff6a09e667f3bcd),
        ("hypot(3,4)", 0x4014000000000000),
        ("fround(0.1)", 0x3fb99999a0000000),
        ("clz32(1)", 0x403f000000000000),
        ("imul(0xffffffff,5)", 0xc014000000000000),
        ("sign(-0)", 0x8000000000000000),
        ("trunc(-0.25)", 0x8000000000000000),
    ] {
        let rt = Runtime::new();
        let code = compile_script("math-libm.js", &format!("Math.{expression}")).unwrap();
        let RunOutcome::Complete(value) = rt.spawn(&code, &[]).unwrap().run().unwrap() else {
            panic!("{expression}")
        };
        let ValueView::Number(value) = value.view() else {
            panic!("{expression}")
        };
        assert_eq!(value.to_bits(), bits, "Math.{expression}");
    }
}

#[test]
fn math_min_max_primitive_work_is_budgeted() {
    const COUNT: usize = 512;
    const SLICE: u64 = 32;
    for (method, result, winner_first) in [
        ("min", -7.0, false),
        ("min", -7.0, true),
        ("max", 7.0, false),
        ("max", 7.0, true),
    ] {
        let rt = Runtime::new();
        // All but the last argument are already on the VM operand stack
        // while need waits. Resuming pushes its result then calls min/max.
        let mut arguments = vec!["0"; COUNT];
        if winner_first {
            arguments[0] = if method == "min" { "-7" } else { "7" };
        }
        let source = format!("Math.{method}({}, __rt.need(319,1))", arguments.join(","));
        let code = compile_script("math-budget.js", &source).unwrap();
        let mut task = rt.spawn(&code, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        let request = task.take_producer_requests().pop().unwrap();
        let value = rt.root(Primitive::Number(if winner_first { 0.0 } else { result }));
        task = rt
            .complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap()
            .into_runnable()
            .pop()
            .unwrap();
        let mut yields = 0;
        let mut done = false;
        for _ in 0..100 {
            match task
                .run_with(Budget::instructions(SLICE), &mut NoEffects)
                .unwrap()
            {
                RunOutcome::Yielded => {
                    yields += 1;
                    rt.collect().unwrap();
                }
                RunOutcome::Complete(value) => {
                    assert_eq!(value.view(), ValueView::Number(result));
                    done = true;
                    break;
                }
                other => panic!("{method}: {other:?}"),
            }
        }
        assert!(done);
        assert!(yields >= COUNT as u64 / SLICE, "{method}: {yields} yields");
    }
}

#[test]
fn math_random_stream_survives_a_wait() {
    let rt = Runtime::new();
    let plain = compile_script(
        "math-random-plain.js",
        "let a = Math.random(); a + ',' + Math.random()",
    )
    .unwrap();
    let RunOutcome::Complete(expected) = rt.spawn(&plain, &[]).unwrap().run().unwrap() else {
        panic!()
    };
    let waiting = compile_script(
        "math-random-wait.js",
        "let a = Math.random(); __rt.need(319,1); a + ',' + Math.random()",
    )
    .unwrap();
    let task = rt.spawn(&waiting, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    let request = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(0.0));
    let task = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    let RunOutcome::Complete(actual) = std::thread::spawn(move || task.run().unwrap())
        .join()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(actual.string(), expected.string());
}
