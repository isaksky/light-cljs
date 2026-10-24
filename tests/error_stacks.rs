//! Error stacks from synthetic JavaScript fixtures, not light-cljs output.
//!
//! The marker helper and position cases adapt get_string_pos and
//! test_line_column_numbers in research/quickjs/tests/test_builtin.js,
//! revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
//!
//! Copyright (c) 2017-2021 Fabrice Bellard
//! Copyright (c) 2017-2021 Charlie Gordon
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//! THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//! THE SOFTWARE.

use clj_quickjs::frontend::compile_script;
use clj_quickjs::{Completion, Primitive, RunOutcome, Runtime, ValueView};

fn string(outcome: RunOutcome) -> String {
    let RunOutcome::Complete(value) = outcome else {
        panic!("expected a string result, got {outcome:?}");
    };
    value.string().expect("a stack string").to_string()
}

fn run(source: &str) -> String {
    let rt = Runtime::new();
    let exe = compile_script("stack-test.js", source).unwrap();
    string(rt.spawn(&exe, &[]).unwrap().run().unwrap())
}

fn marked(source: &str) -> (String, String) {
    let marker = source.find('@').expect("one position marker");
    let before = &source[..marker];
    let line = before.chars().filter(|&c| c == '\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap().chars().count() + 1;
    (source.replacen('@', "", 1), format!(":{line}:{column})"))
}

#[test]
fn error_stack_reports_line_and_column() {
    for (source, caught, level) in [
        ("\n  Error@(\"hello\").stack;", false, 0),
        ("\n  throw Error@(\"hello\");", true, 0),
        ("\n  1 + @not_def", true, 0),
        ("\n  null @( 1234 );", true, 0),
        ("function f() { throw Error('hello'); }\n\nf@();", true, 1),
        (
            "let p = {valueOf() { throw Error('poison'); }};\n  1 + 2 @* p;",
            true,
            1,
        ),
        (
            "let p = {get a() { throw Error('getter'); }};\n  p @. a;",
            true,
            1,
        ),
        (
            "let p = {set a(v) { throw Error('setter'); }};\n  p @. a = 1;",
            true,
            1,
        ),
    ] {
        let (source, position) = marked(source);
        let source = if caught {
            format!("let saved; try {{ {source} }} catch (e) {{ saved = e; }} saved.stack;")
        } else {
            source
        };
        let stack = run(&source);
        let frame = stack.lines().nth(level).expect("the requested frame");
        assert!(
            frame.ends_with(&position),
            "{source:?}: {stack:?}, expected {position}"
        );
        assert!(frame.contains(" (stack-test.js:"), "{stack:?}");
    }
}

#[test]
fn stacks_survive_waits_migration_and_collection() {
    for create in [
        "Error('saved')",
        "(() => { try { missing; } catch(e) { return e; } })()",
    ] {
        let source = format!(
            "function make() {{ return {create}; }}\nlet e = make();\n__rt.need(5, 3); e.stack;"
        );
        let expected = run(&source.replace("__rt.need(5, 3);", ""));
        let rt = Runtime::new();
        let exe = compile_script("stack-test.js", &source).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        let request = task.take_producer_requests().pop().unwrap();
        let ready = rt.root(Primitive::Undefined);
        let mut runnable = rt
            .complete(request.key(), request.producer(), Completion::Ready(&ready))
            .unwrap()
            .into_runnable();
        let task = runnable.pop().unwrap();
        let actual = std::thread::spawn(move || string(task.run().unwrap()))
            .join()
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn stacks_keep_creation_sites_names_and_bounded_depth() {
    let stack = run(
        "function make() { return new Error('saved'); }\nlet e = make();\ntry { throw e; } catch (x) { x.stack; }",
    );
    assert_eq!(
        stack,
        "    at make (stack-test.js:1:26)\n    at <anonymous> (stack-test.js:2:13)\n"
    );
    assert_eq!(
        run("function recur(n) { return n ? recur(n - 1) : Error().stack; } recur(100);")
            .lines()
            .count(),
        64
    );
    let stack = run("const make = () => Error(); make().stack;");
    assert!(stack.starts_with("    at make (stack-test.js:"), "{stack}");
    let stack = run("(function () { return Error().stack; })();");
    assert!(
        stack.starts_with("    at <anonymous> (stack-test.js:"),
        "{stack}"
    );
    // Diagnostics use immutable metadata and never invoke a function's name getter.
    let stack = run(
        "function make() { return Error().stack; } Object.defineProperty(make, 'name', {get() { throw 42; }}); make();",
    );
    assert!(stack.starts_with("    at make (stack-test.js:"), "{stack}");
    // Bound constructors still attribute the Error call to the user's call site.
    assert_eq!(
        run("const E = Error.bind(null);\nnew E().stack;"),
        "    at <anonymous> (stack-test.js:2:1)\n"
    );
}

#[test]
fn stack_property_attributes_follow_error_ownership() {
    for source in [
        "let e = Error(); let d = Object.getOwnPropertyDescriptor(e, 'stack'); d.value === e.stack && d.writable && d.configurable && !d.enumerable && Object.keys(e).length === 0;",
        "let e = Error(); e.stack = 'changed'; e.stack === 'changed' && delete e.stack && e.stack === undefined;",
        "let keys = Object.getOwnPropertyNames(Error('message', {cause: 0})); keys.length === 3 && keys[0] === 'message' && keys[1] === 'cause' && keys[2] === 'stack';",
        "let e; try { missing; } catch (x) { e = x; } let d = Object.getOwnPropertyDescriptor(e, 'stack'); 'stack' in e && d.value === e.stack && !d.writable && !d.configurable && !d.enumerable;",
        "let e; try { missing; } catch (x) { e = x; } let before = e.stack; try { e.stack = 'changed'; } catch (x) {} e.stack === before;",
    ] {
        let rt = Runtime::new();
        let exe = compile_script("stack-test.js", source).unwrap();
        let RunOutcome::Complete(value) = rt.spawn(&exe, &[]).unwrap().run().unwrap() else {
            panic!("{source}");
        };
        assert_eq!(value.view(), ValueView::Bool(true), "{source}");
    }
}
