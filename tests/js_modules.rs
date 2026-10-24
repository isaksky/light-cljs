//! Static loading and module execution; upstream fixtures retain MIT notices.
//! Inline JavaScript is synthetic fixture text, not light-cljs output.

use std::collections::HashMap;

use clj_quickjs::frontend::compile_script;
use clj_quickjs::module::{ModuleHost, ModuleKey, ModuleLoadErrorKind, ModuleLoader, ModuleSource};
use clj_quickjs::{Budget, Host, HostCall, JsString, Primitive, RunOutcome, Runtime, ValueView};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Resolve(JsString, Option<ModuleKey>),
    Fetch(ModuleKey),
}

#[derive(Default)]
struct RecordingHost {
    names: HashMap<String, ModuleKey>,
    sources: HashMap<ModuleKey, (String, Result<String, String>)>,
    events: Vec<Event>,
    effects: Vec<Primitive>,
}

impl Host for RecordingHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        self.effects.push(call.arg);
        Ok(Primitive::Undefined)
    }
}

impl RecordingHost {
    fn add(&mut self, name: &str, text: &str) -> ModuleKey {
        let key = ModuleKey::new(self.names.len() as u64);
        assert!(self.names.insert(name.into(), key).is_none());
        self.sources.insert(key, (name.into(), Ok(text.into())));
        key
    }

    fn fetches(&self) -> Vec<ModuleKey> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Fetch(key) => Some(*key),
                _ => None,
            })
            .collect()
    }
}

impl ModuleHost for RecordingHost {
    fn resolve(
        &mut self,
        specifier: &JsString,
        referrer: Option<ModuleKey>,
    ) -> Result<ModuleKey, String> {
        self.events
            .push(Event::Resolve(specifier.clone(), referrer));
        let specifier =
            String::from_utf16(specifier.code_units()).map_err(|_| "invalid UTF-16 specifier")?;
        let path = if specifier.starts_with("./") || specifier.starts_with("../") {
            let directory =
                referrer.and_then(|key| self.sources[&key].0.rsplit_once('/').map(|(dir, _)| dir));
            directory.map_or_else(|| specifier.clone(), |dir| format!("{dir}/{specifier}"))
        } else {
            specifier
        };
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "." => {}
                ".." if parts.last().is_some_and(|part| *part != "..") => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        self.names
            .get(&parts.join("/"))
            .copied()
            .ok_or_else(|| "absent module".into())
    }

    fn fetch(&mut self, key: ModuleKey) -> Result<ModuleSource, String> {
        self.events.push(Event::Fetch(key));
        let (name, text) = &self.sources[&key];
        Ok(ModuleSource::from_name(name.clone(), text.clone()?))
    }
}

#[test]
fn load_graph_fetches_each_module_once() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let entry = host.add(
        "tests/test_cyclic_import.js",
        include_str!("fixtures/modules/test_cyclic_import.js"),
    );
    let assert = host.add(
        "tests/assert.js",
        include_str!("fixtures/modules/assert.js"),
    );
    let cyclic = host.add(
        "tests/fixture_cyclic_import.js",
        include_str!("fixtures/modules/fixture_cyclic_import.js"),
    );
    let graph = ModuleLoader::new()
        .load(
            runtime.world(),
            &"tests/test_cyclic_import.js".into(),
            &mut host,
        )
        .unwrap();
    assert_eq!(graph.entry(), entry);
    assert_eq!(graph.world(), runtime.world());
    assert_eq!(graph.len(), 3);
    assert_eq!(host.fetches(), vec![entry, assert, cyclic]);
    assert_eq!(graph.get(cyclic).unwrap().requested_modules(), &[entry]);
    assert_eq!(
        graph
            .modules()
            .map(|node| node.module().key())
            .collect::<Vec<_>>(),
        vec![entry, assert, cyclic]
    );
}

#[test]
fn load_reports_unresolvable_specifier() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let entry = host.add(
        "app/entry.js",
        "import './missing.js'; import './later.js'; throw 1;",
    );
    host.add("app/later.js", "throw 2;");
    let error = ModuleLoader::new()
        .load(runtime.world(), &"app/entry.js".into(), &mut host)
        .unwrap_err();
    assert!(matches!(error.kind, ModuleLoadErrorKind::Resolve(_)));
    let message = error.to_string();
    assert!(
        message.contains("ReferenceError: could not load module"),
        "{message}"
    );
    assert!(
        message.contains("./missing.js") && message.contains("app/entry.js"),
        "{message}"
    );
    assert_eq!(host.fetches(), vec![entry]);
    assert_eq!(
        host.events.last(),
        Some(&Event::Resolve("./missing.js".into(), Some(entry)))
    );
}

#[test]
fn load_reports_module_syntax_error_with_position() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add("app/entry.js", "import './bad.js';");
    host.add("app/bad.js", "\nexport const = 1;");
    let error = ModuleLoader::new()
        .load(runtime.world(), &"app/entry.js".into(), &mut host)
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("./bad.js") && message.contains("app/entry.js"),
        "{message}"
    );
    assert!(
        message.contains("app/bad.js:2:14: SyntaxError"),
        "{message}"
    );
    assert!(matches!(error.kind, ModuleLoadErrorKind::Syntax { .. }));
}

#[test]
fn load_carries_json_source_kind() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add(
        "examples/hello_module.js",
        include_str!("fixtures/modules/hello_module.js"),
    );
    host.add(
        "examples/fib_module.js",
        include_str!("fixtures/modules/fib_module.js"),
    );
    let json = host.add("examples/message.json", r#"{"message":"hello"}"#);
    let graph = ModuleLoader::new()
        .load(
            runtime.world(),
            &"examples/hello_module.js".into(),
            &mut host,
        )
        .unwrap();
    let module = graph.get(json).unwrap().module();
    assert_eq!(graph.len(), 3);
    assert!(
        matches!(module.source(), ModuleSource::Json { name, text } if name == "examples/message.json" && text == r#"{"message":"hello"}"#)
    );
    assert!(module.record().is_none());
    assert!(graph.get(json).unwrap().requested_modules().is_empty());
}

#[test]
fn loader_is_not_called_during_task_runs() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add(
        "entry.js",
        "import './dep.js'; throw new Error('must not execute');",
    );
    host.add("dep.js", "__rt.need(9, 2); throw 2;");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    assert_eq!(graph.len(), 2);
    let events = host.events.len();
    // Loading produces no executable yet. Ordinary tasks retain their script API.
    let code = compile_script("script-after-load", "let n = 0; while (n < 5) n++; n").unwrap();
    let task = runtime.spawn(&code, &[]).unwrap();
    let mut complete = false;
    for _ in 0..500 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Number(5.0));
                complete = true;
                break;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    assert!(complete);
    assert_eq!(host.events.len(), events);
    assert!(task.dependency_log().edges().is_empty());
}

#[test]
fn load_reports_fetch_failure_and_stops() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let entry = host.add("entry.js", "import './bad.js'; import './later.js';");
    let bad = host.add("bad.js", "");
    host.sources.get_mut(&bad).unwrap().1 = Err("source unavailable".into());
    host.add("later.js", "");
    let error = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap_err();
    assert!(matches!(error.kind, ModuleLoadErrorKind::Fetch(_)));
    assert!(error.to_string().contains("source unavailable"));
    assert!(error.to_string().contains("./bad.js") && error.to_string().contains("entry.js"));
    assert_eq!(host.fetches(), vec![entry, bad]);
}

#[test]
fn records_are_cached_per_world_and_resolved_key() {
    let runtime = Runtime::new();
    let mut loader = ModuleLoader::new();
    let mut host = RecordingHost::default();
    let entry = host.add(
        "app/entry.js",
        "import './shared.js'; import './sub/../shared.js';",
    );
    let shared = host.add("app/shared.js", "export let value = 1;");
    let first = loader
        .load(runtime.world(), &"app/entry.js".into(), &mut host)
        .unwrap();
    let second = loader
        .load(runtime.world(), &"app/entry.js".into(), &mut host)
        .unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(
        first.get(entry).unwrap().requested_modules(),
        &[shared, shared]
    );
    assert!(std::ptr::eq(
        first.get(shared).unwrap().module(),
        second.get(shared).unwrap().module()
    ));
    assert_eq!(host.fetches(), vec![entry, shared]);
    host.sources.get_mut(&shared).unwrap().1 = Ok("export let value = 2;".into());
    let next = loader
        .load(runtime.new_world(), &"app/entry.js".into(), &mut host)
        .unwrap();
    assert!(!std::ptr::eq(
        first.get(shared).unwrap().module(),
        next.get(shared).unwrap().module()
    ));
    assert_eq!(host.fetches(), vec![entry, shared, entry, shared]);
    assert!(
        next.get(shared)
            .unwrap()
            .module()
            .source()
            .text()
            .contains("2")
    );
    assert!(
        first
            .get(shared)
            .unwrap()
            .module()
            .source()
            .text()
            .contains("1")
    );
}

#[test]
fn load_walks_depth_first_in_request_order() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let entry = host.add("entry.js", "import './a.js'; import './b.js';");
    let a = host.add("a.js", "import './leaf.js';");
    let b = host.add("b.js", "import './leaf.js';");
    let leaf = host.add("leaf.js", "");
    ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    assert_eq!(host.fetches(), vec![entry, a, leaf, b]);
}

#[test]
fn load_accepts_self_import_and_deep_graphs() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    for n in 0..2000 {
        let next = if n == 1999 { 0 } else { n + 1 };
        host.add(&format!("{n}.js"), &format!("import './{next}.js';"));
    }
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"0.js".into(), &mut host)
        .unwrap();
    assert_eq!(graph.len(), 2000);
    assert_eq!(host.fetches().len(), 2000);
    let mut host = RecordingHost::default();
    let entry = host.add("self.js", "import './self.js';");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"self.js".into(), &mut host)
        .unwrap();
    assert_eq!(graph.len(), 1);
    assert_eq!(graph.get(entry).unwrap().requested_modules(), &[entry]);
}

#[test]
fn failed_graph_keeps_records_but_retries_failed_fetch() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let entry = host.add("entry.js", "import './dep.js';");
    let dep = host.add("dep.js", "export let value = 1;");
    host.sources.get_mut(&dep).unwrap().1 = Err("temporary acquisition error".into());
    let mut loader = ModuleLoader::new();
    assert!(
        loader
            .load(runtime.world(), &"entry.js".into(), &mut host)
            .is_err()
    );
    host.sources.get_mut(&dep).unwrap().1 = Ok("export let value = 1;".into());
    let graph = loader
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    assert_eq!(graph.len(), 2);
    assert_eq!(host.fetches(), vec![entry, dep, dep]);
}

#[test]
fn missing_entry_and_lone_surrogate_specifiers_are_reported() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    let error = ModuleLoader::new()
        .load(runtime.world(), &"absent.js".into(), &mut host)
        .unwrap_err();
    assert_eq!(error.referrer, None);
    assert!(
        error
            .to_string()
            .contains("ReferenceError: could not load module 'absent.js'")
    );
    assert!(host.fetches().is_empty());
    host.add("entry.js", r#"import '\uD800';"#);
    let error = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap_err();
    assert_eq!(error.specifier.code_units(), &[0xd800]);
    assert!(
        matches!(host.events.last(), Some(Event::Resolve(specifier, Some(_))) if specifier.code_units() == [0xd800])
    );
}

fn execute_modules(sources: &[(&str, &str)], budget: Budget) -> (RunOutcome, Vec<Primitive>) {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    for (name, text) in sources {
        host.add(name, text);
    }
    let graph = ModuleLoader::new()
        .load(runtime.world(), &sources[0].0.into(), &mut host)
        .unwrap();
    let task = runtime.spawn_module(&graph);
    for _ in 0..100_000 {
        match task.run_with(budget, &mut host).unwrap() {
            RunOutcome::Yielded => {
                runtime.collect().unwrap();
            }
            outcome => return (outcome, host.effects),
        }
    }
    panic!("module task did not finish");
}

fn assert_effects(sources: &[(&str, &str)], expected: Vec<Primitive>) {
    let (outcome, effects) = execute_modules(sources, Budget::UNLIMITED);
    assert!(
        matches!(outcome, RunOutcome::Complete(ref v) if v.view() == ValueView::Undefined),
        "{outcome:?}"
    );
    assert_eq!(effects, expected);
}

#[test]
fn using_module_resources_dispose_after_evaluation() {
    let (outcome, effects) = execute_modules(
        &[
            (
                "entry.js",
                "import {resource} from './resource.js'; __rt.effect(0, resource.value);",
            ),
            (
                "resource.js",
                "using resource = {value: 1, [Symbol.dispose]() { this.value = 2; __rt.effect(0, 10); }}; export {resource}; __rt.effect(0, resource.value);",
            ),
        ],
        Budget::instructions(1),
    );
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(
        effects,
        [
            Primitive::Number(1.0),
            Primitive::Number(10.0),
            Primitive::Number(2.0)
        ]
    );
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import './resource.js'; __rt.effect(0, 99);"),
            (
                "resource.js",
                "using a = {[Symbol.dispose]() { __rt.effect(0, 1); }}, b = {[Symbol.dispose]() { __rt.effect(0, 2); throw 7; }}; throw 8;",
            ),
        ],
        Budget::UNLIMITED,
    );
    assert!(matches!(outcome, RunOutcome::Failed(_)), "{outcome:?}");
    assert_eq!(effects, [Primitive::Number(2.0), Primitive::Number(1.0)]);
}

#[test]
fn named_import_calls_exported_function() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {fib} from './fib.js'; __rt.effect(0, fib(10)); 123;",
            ),
            ("fib.js", include_str!("fixtures/modules/fib_module.js")),
        ],
        vec![Primitive::Number(55.0)],
    );
}

#[test]
fn imports_are_live_bindings() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {n, inc} from './counter.js'; inc(); inc(); __rt.effect(0, n);",
            ),
            (
                "counter.js",
                "export let n = 0; export function inc() { n++ }",
            ),
        ],
        vec![Primitive::Number(2.0)],
    );
}

#[test]
fn import_binding_is_immutable() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {n} from './counter.js'; try { n = 7; } catch (e) { __rt.effect(0, e instanceof TypeError); } __rt.effect(0, n);",
            ),
            ("counter.js", "export let n = 0;"),
        ],
        vec![Primitive::Bool(true), Primitive::Number(0.0)],
    );
}

#[test]
fn reexported_import_resolves_to_origin() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {f as a} from './b.js'; import {f as b} from './c.js'; __rt.effect(0, a === b);",
            ),
            ("b.js", "import {f} from './c.js'; export {f};"),
            ("c.js", "export function f() { return 3; }"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn missing_export_fails_at_link() {
    let (outcome, effects) = execute_modules(
        &[
            (
                "entry.js",
                "import {nope} from './dep.js'; __rt.effect(0, 'entry');",
            ),
            ("dep.js", "__rt.effect(0, 'dep');"),
        ],
        Budget::UNLIMITED,
    );
    let RunOutcome::Failed(error) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(error.kind(), clj_quickjs::VmErrorKind::SyntaxError);
    assert!(
        error
            .message()
            .contains("Could not find export 'nope' in module 'dep.js'"),
        "{error}"
    );
    assert!(effects.is_empty());
}

#[test]
fn evaluation_order_is_post_order() {
    assert_effects(
        &[
            (
                "entry.js",
                "import './a.js'; import './b.js'; __rt.effect(0, 'entry');",
            ),
            ("a.js", "import './leaf.js'; __rt.effect(0, 'a');"),
            ("b.js", "import './leaf.js'; __rt.effect(0, 'b');"),
            ("leaf.js", "__rt.effect(0, 'leaf');"),
        ],
        ["leaf", "a", "b", "entry"]
            .map(|s| Primitive::String(s.into()))
            .to_vec(),
    );
}

#[test]
fn module_body_throw_fails_entry() {
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import './dep.js'; __rt.effect(0, 'entry');"),
            ("dep.js", "throw 42;"),
        ],
        Budget::UNLIMITED,
    );
    let RunOutcome::Failed(error) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(error.kind(), clj_quickjs::VmErrorKind::Thrown);
    assert!(error.message().contains("42"));
    assert!(effects.is_empty());
}

#[test]
fn module_body_yields_and_resumes() {
    let sources = [
        ("entry.js", "import {n} from './dep.js'; __rt.effect(0, n);"),
        (
            "dep.js",
            "export let n = 0; while(n < 12) { __rt.effect(0, n); n++; }",
        ),
    ];
    let (outcome, effects) = execute_modules(&sources, Budget::instructions(7));
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(effects, execute_modules(&sources, Budget::UNLIMITED).1);
}

#[test]
fn module_environments_are_private_and_check_tdz() {
    assert_effects(
        &[(
            "entry.js",
            r#"
        export var moduleVar = 4;
        export function f() { return later; }
        try { f(); } catch(e) { __rt.effect(0, e instanceof ReferenceError); }
        export let later = 7;
        __rt.effect(0, f());
        __rt.effect(0, this === undefined && (() => this)() === undefined);
        __rt.effect(0, globalThis.moduleVar === undefined && globalThis.f === undefined);
        __rt.effect(0, eval('later'));
    "#,
        )],
        vec![
            Primitive::Bool(true),
            Primitive::Number(7.0),
            Primitive::Bool(true),
            Primitive::Bool(true),
            Primitive::Number(7.0),
        ],
    );
}

#[test]
fn import_assignment_through_closure_and_eval_is_immutable() {
    assert_effects(
        &[
            (
                "entry.js",
                r#"import {n} from './dep.js';
            function set() { n = 2; }
            for (const action of [set, () => eval('n = 3')]) {
                try { action(); } catch(e) { __rt.effect(0, e instanceof TypeError); }
            }
            __rt.effect(0, n);
        "#,
            ),
            ("dep.js", "export let n = 1;"),
        ],
        vec![
            Primitive::Bool(true),
            Primitive::Bool(true),
            Primitive::Number(1.0),
        ],
    );
}

#[test]
fn rejects_json_before_effects() {
    for sources in [vec![
        ("entry.js", "import x from './dep.json'; __rt.effect(0, 1);"),
        ("dep.json", "{}"),
    ]] {
        let (outcome, effects) = execute_modules(&sources, Budget::UNLIMITED);
        assert!(
            matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError),
            "{outcome:?}"
        );
        assert!(effects.is_empty());
    }
}

#[test]
fn star_and_explicit_reexports_preserve_identity_and_liveness() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {n, inc, renamed} from './barrel.js'; inc(); __rt.effect(0, n === 1 && renamed === inc);",
            ),
            (
                "barrel.js",
                "export * from './a.js'; export * from './b.js'; export {inc as renamed} from './origin.js';",
            ),
            ("a.js", "export * from './origin.js';"),
            ("b.js", "export * from './origin.js';"),
            (
                "origin.js",
                "export let n = 0; export function inc() { n++; }",
            ),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn ambiguous_star_import_fails_before_any_body() {
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import {n} from './barrel.js';"),
            (
                "barrel.js",
                "export * from './a.js'; export * from './b.js';",
            ),
            ("a.js", "export let n = 1; __rt.effect(0, 'a');"),
            ("b.js", "export let n = 2; __rt.effect(0, 'b');"),
        ],
        Budget::UNLIMITED,
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError && e.message().contains("ambiguous") && e.message().contains("n")),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn explicit_export_overrides_star_ambiguity_and_default_is_excluded() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {n} from './barrel.js'; __rt.effect(0, n);",
            ),
            (
                "barrel.js",
                "export * from './a.js'; export * from './b.js'; export {n} from './a.js';",
            ),
            ("a.js", "export let n = 1; export {n as default};"),
            ("b.js", "export let n = 2;"),
        ],
        vec![Primitive::Number(1.0)],
    );
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import n from './barrel.js';"),
            ("barrel.js", "export * from './a.js';"),
            (
                "a.js",
                "let n = 1; export {n as default}; __rt.effect(0, n);",
            ),
        ],
        Budget::UNLIMITED,
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError && e.message().contains("default")),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn module_need_waits_collects_and_migrates_without_replaying() {
    use clj_quickjs::Completion;
    let runtime = Runtime::new();
    let world = runtime.new_world();
    let mut host = RecordingHost::default();
    host.add("entry.js", "import {n} from './dep.js'; __rt.effect(0, n);");
    host.add(
        "dep.js",
        "export let n = 5; function update() { let x = {value:n}; const g=eval('() => x.value'); eval('x.value += __rt.need(71, 0)'); return g(); } __rt.effect(0, n); n = update(); __rt.effect(0, n);",
    );
    let graph = ModuleLoader::new()
        .load(world, &"entry.js".into(), &mut host)
        .unwrap();
    let mut task = runtime.spawn_module(&graph);
    assert_eq!(task.world(), world);
    let mut waits = 0;
    let mut yields = 0;
    let mut done = false;
    for _ in 0..2000 {
        let (next_task, next_host, outcome) = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let outcome = task.run_with(Budget::instructions(3), &mut host).unwrap();
                (task, host, outcome)
            })
            .unwrap()
            .join()
            .unwrap();
        task = next_task;
        host = next_host;
        runtime.collect().unwrap();
        match outcome {
            RunOutcome::Yielded => {
                yields += 1;
            }
            RunOutcome::Waiting(key) => {
                waits += 1;
                assert_eq!(waits, 1);
                assert_eq!(key.world(), world);
                let request = task.take_producer_requests().pop().unwrap();
                let value = runtime.root(Primitive::Number(7.0));
                task = runtime
                    .complete(key, request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Undefined);
                done = true;
                break;
            }
            outcome => panic!("{outcome:?}"),
        }
    }
    assert!(done && yields > 0);
    assert_eq!(waits, 1);
    assert_eq!(task.dependency_log().edges().len(), 1);
    assert_eq!(
        host.effects,
        vec![
            Primitive::Number(5.0),
            Primitive::Number(12.0),
            Primitive::Number(12.0)
        ]
    );
    assert_eq!(host.fetches().len(), 2);
}

#[test]
fn separate_tasks_get_fresh_instances_and_failed_tasks_cannot_replay() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add(
        "entry.js",
        "import {n, inc} from './dep.js'; inc(); __rt.effect(0, n); throw 42;",
    );
    host.add("dep.js", "export let n = 0; export function inc() { n++; }");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    let first = runtime.spawn_module(&graph);
    let second = runtime.spawn_module(&graph);
    let mut errors = Vec::new();
    for task in [first, second] {
        let RunOutcome::Failed(error) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
            panic!("expected failure")
        };
        errors.push(error.message().to_string());
        assert!(matches!(
            task.run_with(Budget::UNLIMITED, &mut host),
            Err(clj_quickjs::RunError::Finished)
        ));
        assert!(task.dependency_log().edges().is_empty());
    }
    assert_eq!(errors[0], errors[1]);
    assert_eq!(
        host.effects,
        vec![Primitive::Number(1.0), Primitive::Number(1.0)]
    );
}

#[test]
fn unused_missing_indirect_export_fails_at_link() {
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import './dep.js'; __rt.effect(0, 'entry');"),
            ("dep.js", "export {missing} from './leaf.js';"),
            ("leaf.js", "__rt.effect(0, 'leaf');"),
        ],
        Budget::UNLIMITED,
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError && e.message().contains("missing")),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn module_functions_read_reassigned_cells_and_module_eval_rejects_new_target() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {f, replace} from './dep.js'; const old = f; replace(); __rt.effect(0, old());",
            ),
            (
                "dep.js",
                "export function f() { return f === replacement; } function replacement() { return 7; } export function replace() { f = replacement; }",
            ),
        ],
        vec![Primitive::Bool(true)],
    );
    assert_effects(
        &[(
            "entry.js",
            "try { eval('new.target'); } catch(e) { __rt.effect(0, e instanceof SyntaxError); }",
        )],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn deep_acyclic_graph_evaluates_without_nested_module_calls() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    for index in 0..1500 {
        let text = if index == 1499 {
            "__rt.effect(0, 1499);".to_string()
        } else {
            format!("import './{}.js';", index + 1)
        };
        host.add(&format!("{index}.js"), &text);
    }
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"0.js".into(), &mut host)
        .unwrap();
    let task = runtime.spawn_module(&graph);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Complete(_)
    ));
    assert_eq!(host.effects, vec![Primitive::Number(1499.0)]);
}

#[test]
fn module_bindings_are_instantiated_before_body() {
    let sources = [
        ("entry.js", "import {v} from './dep.js'; __rt.effect(0, v);"),
        (
            "dep.js",
            "__rt.effect(0, f()); __rt.effect(0, v === undefined); export function f() { return 7; } export var v = 3;",
        ),
    ];
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        let (outcome, effects) = execute_modules(&sources, budget);
        assert!(
            matches!(outcome, RunOutcome::Complete(ref value) if value.view() == ValueView::Undefined),
            "{outcome:?}"
        );
        assert_eq!(
            effects,
            vec![
                Primitive::Number(7.0),
                Primitive::Bool(true),
                Primitive::Number(3.0)
            ]
        );
    }
}

#[test]
fn module_linking_yields_and_migrates() {
    use clj_quickjs::module::ModulePhase;
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add("entry.js", "import {n} from './0.js'; __rt.effect(0, n);");
    for index in 0..40 {
        host.add(
            &format!("{index}.js"),
            &format!("export * from './{}.js';", index + 1),
        );
    }
    host.add("40.js", "export let n = 7;");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    let mut task = runtime.spawn_module(&graph);
    assert_eq!(task.module_phase(), Some(ModulePhase::Linking));
    let mut linking_slices = 0;
    let mut instantiating = false;
    let mut done = false;
    for _ in 0..3000 {
        let phase = task.module_phase();
        if phase == Some(ModulePhase::Linking) {
            linking_slices += 1;
            assert!(host.effects.is_empty());
        }
        if phase == Some(ModulePhase::Instantiating) {
            instantiating = true;
        }
        let (next, next_host, outcome) = std::thread::spawn(move || {
            let outcome = task.run_with(Budget::instructions(1), &mut host).unwrap();
            (task, host, outcome)
        })
        .join()
        .unwrap();
        task = next;
        host = next_host;
        runtime.collect().unwrap();
        match outcome {
            RunOutcome::Yielded => {}
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Undefined);
                done = true;
                break;
            }
            outcome => panic!("{outcome:?}"),
        }
    }
    assert!(done && instantiating);
    // Every graph edge and every star table entry requires separate linking
    // steps, before any instantiation/body instruction can execute.
    assert!(linking_slices > 200, "{linking_slices}");
    assert_eq!(task.module_phase(), None);
    assert_eq!(host.effects, vec![Primitive::Number(7.0)]);
    assert_eq!(host.fetches().len(), 42);
}

#[test]
fn module_cancellation_stops_linking_and_runs_dependency_finally() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add("entry.js", "import './dep.js'; __rt.effect(0, 'entry');");
    host.add(
        "dep.js",
        "try { __rt.need(73, 0); } finally { __rt.effect(0, 'finally'); }",
    );
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    let linking = runtime.spawn_module(&graph);
    assert!(matches!(
        linking
            .run_with(Budget::instructions(1), &mut host)
            .unwrap(),
        RunOutcome::Yielded
    ));
    let _ = linking.cancel();
    assert!(matches!(
        linking.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Cancelled
    ));
    assert!(host.effects.is_empty());
    let task = runtime.spawn_module(&graph);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    let _ = task.cancel();
    let mut cancelled = false;
    for _ in 0..100 {
        runtime.collect().unwrap();
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Cancelled => {
                cancelled = true;
                break;
            }
            outcome => panic!("{outcome:?}"),
        }
    }
    assert!(cancelled);
    assert_eq!(host.effects, vec![Primitive::String("finally".into())]);
}

#[test]
fn module_linker_cells_retain_and_reclaim() {
    use clj_quickjs::module::ModulePhase;
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add("entry.js", "export let n = 7;");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    let task = runtime.spawn_module(&graph);
    assert_eq!(runtime.heap_stats().objects, 0);
    let mut linked = false;
    for _ in 0..100 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut host).unwrap(),
            RunOutcome::Yielded
        ));
        runtime.collect().unwrap();
        if task.module_phase() == Some(ModulePhase::Instantiating) {
            linked = true;
            break;
        }
    }
    assert!(linked);
    // No module frame has run: the continuation is the cell's only root.
    assert_eq!(runtime.collect().unwrap().live, 1);
    drop(task);
    let collected = runtime.collect().unwrap();
    assert_eq!(collected.reclaimed, 1);
    assert_eq!(collected.live, 0);
}

#[test]
fn cyclic_named_imports_call_hoisted_functions() {
    let sources = [
        (
            "a.js",
            "import {f} from './b.js'; export function g(x) { return x + 1 }; __rt.effect(0, f(1));",
        ),
        (
            "b.js",
            "import {g} from './a.js'; export function f(x) { return 2 * g(x) }",
        ),
    ];
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        let (outcome, effects) = execute_modules(&sources, budget);
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
        assert_eq!(effects, vec![Primitive::Number(4.0)]);
    }
}

#[test]
fn cyclic_let_read_before_init_throws_reference_error() {
    let (outcome, effects) = execute_modules(
        &[
            (
                "a.js",
                "import './b.js'; export let x = 1; __rt.effect(0, 'a');",
            ),
            ("b.js", "import {x} from './a.js'; __rt.effect(0, x);"),
        ],
        Budget::instructions(1),
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::ReferenceError),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn circular_reexport_fails_at_link_with_syntax_error() {
    let (outcome, effects) = execute_modules(
        &[
            (
                "a.js",
                "import {x} from './b.js'; export {x}; __rt.effect(0, 'a');",
            ),
            (
                "b.js",
                "import {x} from './a.js'; export {x}; __rt.effect(0, 'b');",
            ),
        ],
        Budget::instructions(1),
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError && e.message().contains("x") && e.message().contains(".js") && e.message().contains("circular")),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn cycle_evaluates_each_body_once() {
    assert_effects(
        &[
            (
                "a.js",
                "import './b.js'; import './leaf.js'; __rt.effect(0, 'a');",
            ),
            (
                "b.js",
                "import './c.js'; import './leaf.js'; __rt.effect(0, 'b');",
            ),
            (
                "c.js",
                "import './a.js'; import './leaf.js'; __rt.effect(0, 'c');",
            ),
            ("leaf.js", "__rt.effect(0, 'leaf');"),
        ],
        ["leaf", "c", "b", "a"]
            .map(|s| Primitive::String(s.into()))
            .to_vec(),
    );
}

#[test]
fn self_import_uses_own_live_binding() {
    assert_effects(
        &[(
            "self.js",
            "import {f as self, n as ownN} from './self.js'; export function f() { return n; } export let n = 3; __rt.effect(0, self() === ownN);",
        )],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn star_exports_through_cycles_resolve_origins_and_ambiguity() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {n, f} from './a.js'; __rt.effect(0, f() === n);",
            ),
            ("a.js", "export * from './b.js'; export let n = 3;"),
            (
                "b.js",
                "export * from './a.js'; export function f() { return 3; }",
            ),
        ],
        vec![Primitive::Bool(true)],
    );
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import {n} from './a.js'; __rt.effect(0, n);"),
            ("a.js", "export * from './b.js'; export * from './c.js';"),
            ("b.js", "export * from './a.js'; export * from './d.js';"),
            ("c.js", "export let n = 1;"),
            ("d.js", "export let n = 2;"),
        ],
        Budget::instructions(1),
    );
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::SyntaxError && e.message().contains("ambiguous")),
        "{outcome:?}"
    );
    assert!(effects.is_empty());
}

#[test]
fn cycle_body_throw_fails_all_members() {
    let sources = [
        (
            "entry.js",
            "import './a.js'; import './b.js'; __rt.effect(0, 'entry');",
        ),
        ("a.js", "import './b.js'; __rt.effect(0, 'a'); throw 42;"),
        ("b.js", "import './a.js'; __rt.effect(0, 'b');"),
    ];
    let (outcome, effects) = execute_modules(&sources, Budget::instructions(1));
    assert!(
        matches!(outcome, RunOutcome::Failed(ref e) if e.kind() == clj_quickjs::VmErrorKind::Thrown && e.message().contains("42")),
        "{outcome:?}"
    );
    assert_eq!(
        effects,
        ["b", "a"].map(|s| Primitive::String(s.into())).to_vec()
    );
}

#[test]
fn deep_module_chain_links_without_stack_overflow() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    for n in 0..10_000 {
        let text = if n == 9999 {
            "__rt.effect(0, 9999);".into()
        } else {
            format!("import './{}.js';", n + 1)
        };
        host.add(&format!("{n}.js"), &text);
    }
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"0.js".into(), &mut host)
        .unwrap();
    let task = runtime.spawn_module(&graph);
    let outcome = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
            (outcome, host.effects)
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(
        matches!(outcome.0, RunOutcome::Complete(_)),
        "{:?}",
        outcome.0
    );
    assert_eq!(outcome.1, vec![Primitive::Number(9999.0)]);
}

#[test]
fn cyclic_reexported_import_calls_hoisted_function() {
    assert_effects(
        &[
            (
                "a.js",
                "import {f} from './b.js'; export {f}; export function g(x) { return x + 1; } __rt.effect(0, f(1));",
            ),
            (
                "b.js",
                "import {g} from './a.js'; export function f(x) { return 2 * g(x); }",
            ),
        ],
        vec![Primitive::Number(4.0)],
    );
}

#[test]
fn cyclic_import_through_namespace() {
    let (outcome, effects) = execute_modules(
        &[
            (
                "a.js",
                "import {f} from './b.js'; export {f}; export function g(x) { return x + 1; } __rt.effect(0, f(1));",
            ),
            (
                "b.js",
                "import * as a from './a.js'; export function f(x) { return 2 * a.g(x); }",
            ),
        ],
        Budget::UNLIMITED,
    );
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(effects, vec![Primitive::Number(4.0)]);
}

#[test]
fn cycle_waits_collects_and_migrates_without_replaying() {
    use clj_quickjs::Completion;
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add(
        "a.js",
        "import {f} from './b.js'; export function g(x) { return x + 1; } __rt.effect(0, f(1));",
    );
    host.add("b.js", "import {g} from './a.js'; export function f(x) { return 2 * g(x); } __rt.effect(0, 'b'); __rt.need(81, 0);");
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"a.js".into(), &mut host)
        .unwrap();
    let mut task = runtime.spawn_module(&graph);
    let mut waits = 0;
    for _ in 0..2000 {
        let (next, next_host, outcome) = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let outcome = task.run_with(Budget::instructions(1), &mut host).unwrap();
                (task, host, outcome)
            })
            .unwrap()
            .join()
            .unwrap();
        task = next;
        host = next_host;
        runtime.collect().unwrap();
        match outcome {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(key) => {
                waits += 1;
                assert_eq!(waits, 1);
                let request = task.take_producer_requests().pop().unwrap();
                let value = runtime.root(Primitive::Undefined);
                task = runtime
                    .complete(key, request.producer(), Completion::Ready(&value))
                    .unwrap()
                    .into_runnable()
                    .pop()
                    .unwrap();
            }
            RunOutcome::Complete(_) => {
                assert_eq!(waits, 1);
                assert_eq!(
                    host.effects,
                    vec![Primitive::String("b".into()), Primitive::Number(4.0)]
                );
                assert_eq!(host.fetches().len(), 2);
                assert_eq!(task.dependency_log().edges().len(), 1);
                return;
            }
            outcome => panic!("{outcome:?}"),
        }
    }
    panic!("cycle did not complete");
}

#[test]
fn cyclic_exports_resolve_later_ancestor_dependencies() {
    assert_effects(
        &[
            (
                "a.js",
                "import {n} from './b.js'; export * from './leaf.js'; __rt.effect(0, n);",
            ),
            ("b.js", "export {n} from './a.js';"),
            ("leaf.js", "export let n = 7;"),
        ],
        vec![Primitive::Number(7.0)],
    );
}

#[test]
fn deep_cyclic_reexports_resolve_without_stack_overflow() {
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    for n in 0..1000 {
        let text = if n == 999 {
            "import './0.js'; export function f() { return 7; }".into()
        } else if n == 0 {
            "import {f} from './1.js'; export {f}; __rt.effect(0, f());".into()
        } else {
            format!("export {{f}} from './{}.js';", n + 1)
        };
        host.add(&format!("{n}.js"), &text);
    }
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"0.js".into(), &mut host)
        .unwrap();
    let task = runtime.spawn_module(&graph);
    let (outcome, effects) = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || {
            let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
            (outcome, host.effects)
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(effects, vec![Primitive::Number(7.0)]);
}

#[test]
fn cyclic_dependency_calls_ancestor_function_before_body() {
    let sources = [
        (
            "a.js",
            "import './b.js'; export function g(x) { return x + 1; }",
        ),
        ("b.js", "import {g} from './a.js'; __rt.effect(0, g(1));"),
    ];
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        let (outcome, effects) = execute_modules(&sources, budget);
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
        assert_eq!(effects, vec![Primitive::Number(2.0)]);
    }
}

#[test]
fn cyclic_star_export_skips_back_edge_and_uses_later_origin() {
    let sources = [
        ("entry.js", "import {n} from './a.js'; __rt.effect(0, n);"),
        ("a.js", "export * from './b.js'; export * from './leaf.js';"),
        ("b.js", "export * from './a.js';"),
        ("leaf.js", "export let n = 7;"),
    ];
    for budget in [Budget::UNLIMITED, Budget::instructions(1)] {
        let (outcome, effects) = execute_modules(&sources, budget);
        assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
        assert_eq!(effects, vec![Primitive::Number(7.0)]);
    }
}

#[test]
fn namespace_import_reads_exports() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; __rt.effect(0, m.g(1) === 2 && m.g() === m && m.missing === undefined);",
            ),
            (
                "m.js",
                "export function g(x) { if (x === undefined) return this; return x + 1; }",
            ),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_reads_are_live() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; const d = Object.getOwnPropertyDescriptor(m, 'n'); m.inc(); __rt.effect(0, d.value === 0 && m.n === 1 && Object.getOwnPropertyDescriptor(m, 'n').value === 1);",
            ),
            ("m.js", "export let n = 0; export function inc() { n++; }"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_identity_is_per_instance() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; import {ns as a} from './a.js'; import {ns as b} from './b.js'; __rt.effect(0, m === a && a === b && a.n === 7);",
            ),
            ("a.js", "import * as ns from './m.js'; export {ns};"),
            ("b.js", "export * as ns from './m.js';"),
            ("m.js", "export let n = 7;"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_is_read_only() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; let errors=0; try { m.g=1; } catch(e) { if(e instanceof TypeError) errors++; } try { m.newProp=1; } catch(e) { if(e instanceof TypeError) errors++; } try { delete m.g; } catch(e) { if(e instanceof TypeError) errors++; } try { m=1; } catch(e) { if(e instanceof TypeError) errors++; } __rt.effect(0, errors === 4 && delete m.absent);",
            ),
            ("m.js", "export function g() {}"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_reflection_and_definitions() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; const d=Object.getOwnPropertyDescriptor(m, 'a'); const tag=Object.getOwnPropertyDescriptor(m, Symbol.toStringTag); const keys=Object.getOwnPropertyNames(m); let errors=0; try { Object.defineProperty(m,'a',{value:2}); } catch(e) { if(e instanceof TypeError) errors++; } try { Object.defineProperty(m,'a',{writable:false}); } catch(e) { if(e instanceof TypeError) errors++; } try { Object.defineProperty(m,'a',{get(){return 1;}}); } catch(e) { if(e instanceof TypeError) errors++; } try { Object.defineProperty(m,'absent',{value:1}); } catch(e) { if(e instanceof TypeError) errors++; } try { Object.setPrototypeOf(m,{}); } catch(e) { if(e instanceof TypeError) errors++; } Object.setPrototypeOf(m,null); Object.preventExtensions(m); Object.defineProperty(m,'a',{value:1,writable:true,enumerable:true,configurable:false}); Object.defineProperty(m,Symbol.toStringTag,{value:'Module'}); __rt.effect(0, errors===5 && Object.getPrototypeOf(m)===null && !Object.isExtensible(m) && !Object.isFrozen(m) && Object.keys(m).join(',')==='a,b,z' && keys.length===3 && 'a' in m && !('missing' in m) && m[Symbol.toStringTag]==='Module' && d.writable && d.enumerable && !d.configurable && !tag.writable && !tag.enumerable && !tag.configurable && Object.prototype.toString.call(m)==='[object Module]');",
            ),
            ("m.js", "export let z=3, a=1, b=2;"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_sorts_utf16_and_omits_ambiguous_stars() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; __rt.effect(0, Object.keys(m).join(',')==='a,𐀀,ﬀ' && !('conflict' in m));",
            ),
            (
                "m.js",
                "export * from './a.js'; export * from './b.js'; export {a as 𐀀, a as ﬀ}; let a=1; export {a};",
            ),
            ("a.js", "let a=1; export {a as conflict};"),
            ("b.js", "let b=2; export {b as conflict};"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_reexport_paths_share_one_origin() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {ns} from './barrel.js'; import * as m from './m.js'; __rt.effect(0, ns===m);",
            ),
            (
                "barrel.js",
                "export * from './a.js'; export * from './b.js';",
            ),
            ("a.js", "import * as ns from './m.js'; export {ns};"),
            ("b.js", "export * as ns from './m.js';"),
            ("m.js", ""),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_empty_freezes_and_inherited_writes_fail() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; import * as empty from './empty.js'; let errors=0; try { Object.freeze(m); } catch(e) { if(e instanceof TypeError) errors++; } const o=Object.create(m); try { o.newProp=1; } catch(e) { if(e instanceof TypeError) errors++; } Object.defineProperty(o,'n',{value:9}); __rt.effect(0, errors===2 && o.n===9 && m.n===1 && Object.freeze(empty)===empty && Object.isFrozen(empty));",
            ),
            ("m.js", "export let n=1;"),
            ("empty.js", ""),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_waits_collects_and_migrates() {
    use clj_quickjs::Completion;
    let runtime = Runtime::new();
    let mut host = RecordingHost::default();
    host.add("entry.js", "import * as m from './m.js'; import {ns} from './barrel.js'; __rt.effect(0, m===ns && m.n===12); m.inc(); __rt.effect(0, ns.n===13);");
    host.add("barrel.js", "export * as ns from './m.js';");
    host.add(
        "m.js",
        "export let n=5; export function inc(){n++;} n+=__rt.need(75,0);",
    );
    let graph = ModuleLoader::new()
        .load(runtime.world(), &"entry.js".into(), &mut host)
        .unwrap();
    // Each spawn gets fresh cells and namespaces even though records are cached.
    for _ in 0..2 {
        let mut task = runtime.spawn_module(&graph);
        let mut done = false;
        for _ in 0..1500 {
            let (next, next_host, outcome) = std::thread::Builder::new()
                .stack_size(64 * 1024)
                .spawn(move || {
                    let outcome = task.run_with(Budget::instructions(1), &mut host).unwrap();
                    (task, host, outcome)
                })
                .unwrap()
                .join()
                .unwrap();
            task = next;
            host = next_host;
            runtime.collect().unwrap();
            match outcome {
                RunOutcome::Yielded => {}
                RunOutcome::Waiting(key) => {
                    let request = task.take_producer_requests().pop().unwrap();
                    let value = runtime.root(Primitive::Number(7.0));
                    task = runtime
                        .complete(key, request.producer(), Completion::Ready(&value))
                        .unwrap()
                        .into_runnable()
                        .pop()
                        .unwrap();
                }
                RunOutcome::Complete(_) => {
                    done = true;
                    break;
                }
                outcome => panic!("{outcome:?}"),
            }
        }
        assert!(done);
        assert_eq!(task.dependency_log().edges().len(), 1);
        drop(task);
        runtime.collect().unwrap();
        assert_eq!(runtime.heap_usage().objects.module_namespace, 0);
    }
    assert_eq!(host.effects, vec![Primitive::Bool(true); 4]);
    assert_eq!(host.fetches().len(), 3);
}

#[test]
fn namespace_default_tag_and_enumeration() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; import d from './barrel.js'; let names=''; for(let k in m) names+=k+','; let errors=0; try{delete m[Symbol.toStringTag];}catch(e){if(e instanceof TypeError)errors++;} try{m[Symbol.toStringTag]='x';}catch(e){if(e instanceof TypeError)errors++;} try{Object.defineProperty(m,Symbol.toStringTag,{configurable:true});}catch(e){if(e instanceof TypeError)errors++;} globalThis.ns=m; (0,eval)('ns.n=999; ns.absent=1;'); m.inc(); __rt.effect(0, errors===3 && m.n===2 && m.default===2 && m.absent===undefined && names==='default,inc,n,' && d===m && JSON.stringify(m)==='{\"default\":2,\"n\":2}');",
            ),
            ("barrel.js", "export * as default from './m.js';"),
            (
                "m.js",
                "export let n=1; export {n as default}; export function inc(){n++;}",
            ),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_get_in_tdz_throws_reference_error() {
    let (outcome, effects) = execute_modules(
        &[
            ("entry.js", "import './reader.js'; export let x=1;"),
            (
                "reader.js",
                "import * as m from './entry.js'; try { m.x; } catch(e) { __rt.effect(0,e instanceof ReferenceError); }",
            ),
        ],
        Budget::UNLIMITED,
    );
    assert!(matches!(outcome, RunOutcome::Complete(_)), "{outcome:?}");
    assert_eq!(effects, vec![Primitive::Bool(true)]);
}

#[test]
fn namespace_definitions_use_same_value() {
    assert_effects(
        &[
            (
                "entry.js",
                "import * as m from './m.js'; Object.defineProperty(m,'nan',{value:NaN}); Object.defineProperty(m,'zero',{value:-0}); const o=m.obj; Object.defineProperty(m,'obj',{value:o}); let errors=0; try { Object.defineProperty(m,'zero',{value:0}); }catch(e){if(e instanceof TypeError)errors++;} try { Object.defineProperty(m,'obj',{value:{}}); }catch(e){if(e instanceof TypeError)errors++;} __rt.effect(0, errors===2 && Object.is(m.zero,-0) && m.obj===o);",
            ),
            ("m.js", "export let nan=NaN, zero=-0, obj={};"),
        ],
        vec![Primitive::Bool(true)],
    );
}

#[test]
fn namespace_escaped_reexport_names() {
    assert_effects(
        &[
            (
                "entry.js",
                "import {if as ns} from './barrel.js'; __rt.effect(0, ns.n===7);",
            ),
            ("barrel.js", r"export * as \u0069f from './m.js';"),
            ("m.js", "export let n=7;"),
        ],
        vec![Primitive::Bool(true)],
    );
}
