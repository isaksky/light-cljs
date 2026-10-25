//! Iterable consumer shared by Map/WeakMap, Set/WeakSet and Object.fromEntries.
//! Each observable read/call resumes through the trampoline; entry validation
//! is a separate budgeted step so IteratorStepValue failures do not close.
use super::*;
use crate::vm::exception::Exception;

#[derive(Debug)]
enum Stage {
    Start,
    Adder,
    Iterator,
    Next,
    NextResult,
    Done,
    Value,
    Entry,
    Key,
    EntryValue,
    Add,
    PropertyKey,
    Added,
    Close,
    CloseMethod,
    CloseResult,
    Rethrow,
}

#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    values: bool,
    from_entries: bool,
    target: Value,
    items: Value,
    adder: Value,
    acquisition: Option<iterator::State>,
    iterator: Value,
    next: Value,
    result: Value,
    key: Value,
    pending: Option<Exception>,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for value in [
            &self.target,
            &self.items,
            &self.adder,
            &self.iterator,
            &self.next,
            &self.result,
            &self.key,
        ] {
            value.trace(visit);
        }
        if let Some(acquisition) = &self.acquisition {
            acquisition.trace(visit);
        }
        if let Some(pending) = &self.pending {
            pending.value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(target: Value, items: Value) -> Self {
        Self {
            stage: Stage::Start,
            values: false,
            from_entries: false,
            target,
            items,
            adder: Value::Undefined,
            acquisition: None,
            iterator: Value::Undefined,
            next: Value::Undefined,
            result: Value::Undefined,
            key: Value::Undefined,
            pending: None,
        }
    }

    pub(super) fn consume_values(&mut self) {
        self.values = true;
    }

    pub(super) fn from_entries(target: Value, items: Value) -> Self {
        Self {
            from_entries: true,
            ..Self::new(target, items)
        }
    }

    pub(super) fn catches(&self) -> bool {
        matches!(
            self.stage,
            Stage::Entry
                | Stage::Key
                | Stage::EntryValue
                | Stage::Add
                | Stage::PropertyKey
                | Stage::Added
                | Stage::Close
                | Stage::CloseMethod
                | Stage::CloseResult
        )
    }

    // These steps start without a delivered operand, so NativeCatch restores
    // exactly the native's base, including when an immediate property read fails.
    pub(super) fn catches_step(&self) -> bool {
        matches!(self.stage, Stage::Entry | Stage::EntryValue | Stage::Close)
    }

    pub(super) fn failed(&mut self, exception: Exception) {
        if self.pending.is_none() {
            self.pending = Some(exception);
            self.stage = Stage::Close;
        } else {
            self.stage = Stage::Rethrow;
        }
    }

    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(value) => Ok(value),
            _ => Err(Fault::Internal(
                "AddEntriesFromIterable expected call result",
            )),
        }
    }

    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        key: property::Key,
        stage: Stage,
    ) -> Result<Action, Fault> {
        self.stage = stage;
        match property::read(heap, task, &base, &key)? {
            Some(property::Read::Getter(callee)) => Ok(Action::Call {
                callee,
                this: base,
                args: vec![],
            }),
            Some(property::Read::Value(value)) => self.step(heap, task, Input::CallResult(value)),
            None => self.step(heap, task, Input::CallResult(Value::Undefined)),
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Start => {
                if self.from_entries {
                    self.acquisition = Some(iterator::State::get_iterator(self.items.clone()));
                    self.items = Value::Undefined;
                    self.stage = Stage::Iterator;
                    return Ok(Action::Continue);
                }
                if matches!(self.items, Value::Null | Value::Undefined) {
                    return Ok(Action::Return(self.target.clone()));
                }
                self.read(
                    heap,
                    task,
                    self.target.clone(),
                    property::Key::Name(JsString::from(if self.values { "add" } else { "set" })),
                    Stage::Adder,
                )
            }
            Stage::Adder => {
                self.adder = Self::value(input)?;
                dispatch::callee(heap, &self.adder)?;
                self.acquisition = Some(iterator::State::get_iterator(self.items.clone()));
                self.items = Value::Undefined;
                self.stage = Stage::Iterator;
                Ok(Action::Continue)
            }
            Stage::Iterator => {
                let action = self
                    .acquisition
                    .as_mut()
                    .ok_or(Fault::Internal("missing GetIterator"))?
                    .step(heap, task, input)?;
                if let Action::Return(record) = action {
                    self.iterator =
                        property::get_field(heap, task, &record, &JsString::from("iterator"))?;
                    self.next = property::get_field(heap, task, &record, &JsString::from("next"))?;
                    self.acquisition = None;
                    self.stage = Stage::Next;
                    Ok(Action::Continue)
                } else {
                    Ok(action)
                }
            }
            Stage::Next => {
                self.stage = Stage::NextResult;
                Ok(Action::Call {
                    callee: self.next.clone(),
                    this: self.iterator.clone(),
                    args: vec![],
                })
            }
            Stage::NextResult => {
                self.result = Self::value(input)?;
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.read(
                    heap,
                    task,
                    self.result.clone(),
                    property::Key::Name(JsString::from("done")),
                    Stage::Done,
                )
            }
            Stage::Done => {
                if Self::value(input)?.truthy() {
                    return Ok(Action::Return(self.target.clone()));
                }
                self.read(
                    heap,
                    task,
                    self.result.clone(),
                    property::Key::Name(JsString::from("value")),
                    Stage::Value,
                )
            }
            Stage::Value => {
                self.result = Self::value(input)?;
                self.stage = Stage::Entry;
                Ok(Action::Continue)
            }
            Stage::Entry => {
                if self.values {
                    self.stage = Stage::Added;
                    return Ok(Action::Call {
                        callee: self.adder.clone(),
                        this: self.target.clone(),
                        args: vec![self.result.clone()],
                    });
                }
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator entry must be an object".into()));
                }
                self.read(
                    heap,
                    task,
                    self.result.clone(),
                    property::Key::Index(0),
                    Stage::Key,
                )
            }
            Stage::Key => {
                self.key = Self::value(input)?;
                self.stage = Stage::EntryValue;
                Ok(Action::Continue)
            }
            Stage::EntryValue => self.read(
                heap,
                task,
                self.result.clone(),
                property::Key::Index(1),
                Stage::Add,
            ),
            Stage::Add => {
                let value = Self::value(input)?;
                self.stage = Stage::Added;
                if self.from_entries {
                    self.result = value;
                    self.stage = Stage::PropertyKey;
                    return Ok(Action::Instruction {
                        // Unary ToPropertyKey, preserving Symbols and using string hint.
                        instr: Instr::ToSuperKey,
                        operands: vec![self.key.clone()],
                    });
                }
                Ok(Action::Call {
                    callee: self.adder.clone(),
                    this: self.target.clone(),
                    args: vec![self.key.clone(), value],
                })
            }
            Stage::PropertyKey => {
                self.key = Self::value(input)?;
                self.stage = Stage::Added;
                Ok(Action::Instruction {
                    instr: Instr::DefineElem,
                    operands: vec![self.target.clone(), self.key.clone(), self.result.clone()],
                })
            }
            Stage::Added => {
                self.key = Value::Undefined;
                self.result = Value::Undefined;
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::Close => self.read(
                heap,
                task,
                self.iterator.clone(),
                property::Key::Name(JsString::from("return")),
                Stage::CloseMethod,
            ),
            Stage::CloseMethod => {
                let method = Self::value(input)?;
                if matches!(method, Value::Null | Value::Undefined) {
                    self.stage = Stage::Rethrow;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::CloseResult;
                    Ok(Action::Call {
                        callee: method,
                        this: self.iterator.clone(),
                        args: vec![],
                    })
                }
            }
            Stage::CloseResult => {
                // The original throw wins over return's result or exception.
                self.stage = Stage::Rethrow;
                Ok(Action::Continue)
            }
            Stage::Rethrow => {
                Err(Fault::Rethrow(self.pending.take().ok_or(
                    Fault::Internal("missing AddEntriesFromIterable exception"),
                )?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::compile_script;
    use crate::heap::gc::{RootCategory, RootSource, TraceEdge};
    use crate::host::NoHost;
    use crate::ids::WorldId;
    use crate::vm::dispatch::{Exit, execute};
    use crate::vm::frames::{DiagnosticContext, VmLimits};
    use std::sync::{Arc, Mutex};

    impl RootSource for Mutex<VmState> {
        fn trace_roots(&self, visit: &mut dyn FnMut(RootCategory, TraceEdge)) {
            let vm = self.lock().unwrap();
            vm.trace(&mut |edge| visit(RootCategory::TaskFrame, edge));
            vm.top()
                .code
                .trace(&mut |edge| visit(RootCategory::ExecutableConstant, edge));
        }
    }

    // Realm prototypes are immutable, and built-in subclasses are -309's
    // scope. At the first explicit Need, inject the shared consumer with a
    // task-owned target whose own set can be overridden. All reads/callbacks
    // still execute from source through the ordinary budgeted trampoline.
    fn check(source: &str) {
        check_consumer(source, false);
    }

    fn check_consumer(source: &str, values: bool) {
        let heap = Heap::default();
        let code = compile_script("add-entries.js", source).unwrap();
        let task = TaskId::from_raw(294);
        let diag = DiagnosticContext {
            task,
            world: WorldId::INITIAL,
        };
        let saved = Arc::new(Mutex::new(VmState::new(
            &code.0,
            &[],
            diag,
            VmLimits::DEFAULT,
        )));
        let roots: Arc<dyn RootSource> = saved.clone();
        heap.register_source(Arc::downgrade(&roots));
        let mut injected = false;
        for _ in 0..10_000 {
            let mut vm = saved.lock().unwrap();
            match execute(&mut vm, &heap, &mut NoHost, Some(1)) {
                Exit::Yield => {
                    drop(vm);
                    heap.collect().unwrap();
                }
                Exit::Need { dependency, .. }
                    if dependency == crate::ids::DependencyId::new(294) =>
                {
                    let global = Value::Object(vm.global_object.unwrap());
                    let target =
                        property::get_field(&heap, task, &global, &JsString::from("target"))
                            .unwrap_or_else(|_| panic!("fixture field"));
                    let items = property::get_field(&heap, task, &global, &JsString::from("items"))
                        .unwrap_or_else(|_| panic!("fixture field"));
                    if values {
                        start_add_values(&mut vm, target, items, 0)
                    } else {
                        start_add_entries(&mut vm, target, items, 0)
                    }
                    .unwrap_or_else(|_| panic!("start consumer"));
                    injected = true;
                }
                Exit::Need { .. } => vm.push(Value::Number(7.0)),
                Exit::Return(value) => {
                    assert!(injected);
                    assert_eq!(value, Value::Bool(true), "{source}");
                    return;
                }
                other => panic!("{other:?}: {source}"),
            }
        }
        panic!("consumer exceeded instruction bound");
    }

    /// Synthetic fixtures, not light-cljs output.
    #[test]
    fn caches_adder_before_iterator_and_resumes_user_set() {
        for constructor in ["Map", "WeakMap"] {
            check(&format!(
                r#"
                let C = {constructor}, original = C.prototype.set, log = '', receiver;
                globalThis.target = new C();
                let k = {{}}, n = 0;
                Object.defineProperty(target, 'set', {{configurable: true, get() {{
                    log += 's'; receiver = this; __rt.need(5, 3);
                    return function(key, value) {{
                        log += 'a'; __rt.need(5, 3);
                        Object.defineProperty(target, 'set', {{value: 0, configurable: true}});
                        if (this !== receiver || arguments.length !== 2) throw 99;
                        return original.call(this, key, value);
                    }};
                }} }});
                globalThis.items = {{
                    get [Symbol.iterator]() {{ log += 'i'; return function() {{ log += 'I'; return this; }}; }},
                    get next() {{ log += 'n'; return function() {{
                        log += 'N';
                        if (++n > 2) return {{done: true, get value() {{ throw 98; }} }};
                        return {{get done() {{ log += 'd'; return false; }}, get value() {{
                            log += 'v'; return {{get 0() {{ log += 'k'; return k; }}, get 1() {{ log += 'w'; return n; }} }};
                        }} }};
                    }}; }}
                }};
                let m = __rt.need(294, 0);
                m === target && m === receiver && m.get(k) === 2 && log === 'siInNdvkwaNdvkwaN'
            "#
            ));
        }
    }

    #[test]
    fn adder_validation_precedes_acquisition_and_nullish_skips_lookup() {
        for items in [
            "null",
            "undefined",
            "({get [Symbol.iterator]() { reads += 100; }})",
        ] {
            check(&format!(
                r#"
                let reads = 0, ok = false;
                globalThis.target = {{get set() {{ reads++; throw 17; }} }};
                globalThis.items = {items};
                try {{ ok = __rt.need(294, 0) === target; }} catch(e) {{ ok = e === 17; }}
                ok && reads === {expected}
            "#,
                expected = usize::from(items.starts_with('('))
            ));
        }
        check(
            r#"
            let reads = 0, ok = false;
            globalThis.target = {set: 0};
            globalThis.items = {get [Symbol.iterator]() { reads++; }};
            try { __rt.need(294, 0); } catch(e) { ok = e instanceof TypeError; }
            ok && reads === 0
        "#,
        );
    }

    #[test]
    fn set_throw_closes_once_and_preserves_exception() {
        for close in [
            "return() { closed++; __rt.need(5,3); return {}; }",
            "return() { closed++; throw 99; }",
            "return() { closed++; return 1; }",
            "get return() { closed++; __rt.need(5,3); throw 99; }",
            "return: 0",
            "return: null",
        ] {
            check(&format!(
                r#"
                let closed = 0, error = {{tag: 17}}, ok = false, calls = 0;
                globalThis.target = {{set() {{ calls++; __rt.need(5,3); throw error; }} }};
                globalThis.items = {{[Symbol.iterator]() {{return this;}}, next() {{return {{value: [{{}}, 2]}};}}, {close} }};
                try {{ __rt.need(294, 0); }} catch(e) {{ ok = e === error; }}
                ok && calls === 1 && closed === {expected}
            "#,
                expected = usize::from(close.contains("closed++"))
            ));
        }
    }

    #[test]
    fn native_roots_intermediate_key_and_pending_throw() {
        check(
            r#"
            globalThis.target = {set(k, v) { this.ok = k.n === 17 && v === 7; }};
            globalThis.items = [{get 0() { return {n:17}; }, get 1() { return __rt.need(5,3); }}];
            let m = __rt.need(294, 0);
            m.ok
        "#,
        );
        check(
            r#"
            let closed = 0, ok = false;
            globalThis.target = {set() { throw {tag:17}; }};
            globalThis.items = {
                [Symbol.iterator]() { return this; },
                next() { return {value:[{},2]}; },
                return() { closed++; __rt.need(5,3); throw {tag:99}; }
            };
            try { __rt.need(294, 0); } catch(e) { ok = e.tag === 17; }
            ok && closed === 1
        "#,
        );
    }

    #[test]
    fn values_cache_add_before_iterator_and_resume_calls() {
        for constructor in ["Set", "WeakSet"] {
            check_consumer(
                &format!(
                    r#"
                let C = {constructor}, original = C.prototype.add, log = '', k = {{}}, n = 0;
                globalThis.target = new C();
                Object.defineProperty(target, 'add', {{configurable: true, get() {{
                    log += 'a'; __rt.need(5,3);
                    return function(value) {{
                        log += 'A'; __rt.need(5,3);
                        Object.defineProperty(target, 'add', {{value: 0, configurable: true}});
                        if (this !== target || arguments.length !== 1) throw 99;
                        return original.call(this, value);
                    }};
                }} }});
                globalThis.items = {{get [Symbol.iterator]() {{
                    log += 'i'; return function() {{return this;}};
                }}, next() {{return ++n > 2 ? {{done:true}} : {{value:k}};}} }};
                let s = __rt.need(294,0);
                s === target && s.has(k) && log === 'aiAA'
            "#
                ),
                true,
            );
        }
    }

    #[test]
    fn values_add_failures_close_and_root_original_exception() {
        for close in [
            "return() {closed++; __rt.need(5,3); return {};}",
            "get return() {closed++; __rt.need(5,3); throw 99;}",
            "return() {closed++; throw 99;}",
        ] {
            check_consumer(
                &format!(
                    r#"
                let closed = 0, ok = false;
                globalThis.target = {{add(v) {{__rt.need(5,3); throw {{tag:v.n}};}}}};
                globalThis.items = {{[Symbol.iterator]() {{return this;}},
                    next() {{return {{value:{{n:17}}}};}}, {close} }};
                try {{__rt.need(294,0);}} catch(e) {{ok = e.tag === 17;}}
                ok && closed === 1
            "#
                ),
                true,
            );
        }
        check_consumer(
            "let reads = 0, ok = false; globalThis.target = {add:0}; globalThis.items = {get [Symbol.iterator]() {reads++;}}; try {__rt.need(294,0);} catch(e) {ok = e instanceof TypeError;} ok && reads === 0",
            true,
        );
        check_consumer(
            "globalThis.target = {get add() {throw 99;}}; globalThis.items = null; __rt.need(294,0) === target",
            true,
        );
    }
}
