//! Resumable String/RegExp symbol protocols. Every observable property access,
//! conversion, call and write returns to the VM; matcher leaves retain their
//! existing bound. Strings are checked before flattening, and buffers/work are
//! limited across the whole operation, including custom exec implementations.
use super::{Action, Input, string_replace};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::regexp::{matcher, object};
use crate::value::{JsString, Value, symbol};
use crate::vm::{
    builtin::{self, Builtin},
    dispatch::{self, Fault},
    property,
};

const MAX_UNITS: usize = 65_536;
const MAX_WORK: usize = 1_000_000;

pub(super) fn handles(which: Builtin) -> bool {
    use Builtin::*;
    matches!(
        which,
        StringMatch
            | StringMatchAll
            | StringPrototypeSearch
            | StringPrototypeReplace
            | StringPrototypeReplaceAll
            | StringPrototypeSplit
            | RegExpMatch
            | RegExpMatchAll
            | RegExpSearch
            | RegExpReplace
            | RegExpSplit
            | RegExpIteratorNext
    )
}

#[derive(Clone, Debug)]
pub(crate) struct IteratorData {
    regexp: Value,
    text: JsString,
    global: bool,
    unicode: bool,
    done: bool,
}
impl Trace for IteratorData {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.regexp.trace(visit);
    }
}
fn cursor<R>(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    f: impl FnOnce(&mut IteratorData) -> R,
) -> Result<R, Fault> {
    let invalid = || Fault::TypeError("next requires a RegExp String Iterator".into());
    let Value::Object(id) = this else {
        return Err(invalid());
    };
    heap.with_object_mut(*id, |o| {
        let HeapObject::Plain(o) = o else {
            return Err(invalid());
        };
        let owner = o.owner();
        let data = o.regexp_iterator().ok_or_else(invalid)?;
        if owner != Some(task) {
            return Err(Fault::ForeignObject {
                owner,
                caller: task,
            });
        }
        Ok(f(data))
    })
    .ok_or_else(|| Fault::Internal("RegExp iterator reclaimed"))?
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Dispatch,
    IsRegExp,
    CheckFlags,
    CheckFlagsString,
    Method,
    Invoke,
    Return,
    FallbackText,
    FallbackCreated,
    FallbackMethod,
    SplitLimit,
    SplitLimitNumber,
    SplitSeparator,
    SplitSeparatorString,
    StringSplit,
    Start,
    Text,
    Replacement,
    Flags,
    FlagsString,
    Constructor,
    Species,
    SpeciesValue,
    SpeciesCheck,
    SpeciesFlags,
    SpeciesFlagsString,
    Created,
    CopyIndex,
    CopyIndexNumber,
    IteratorCreate,
    SearchIndex,
    SearchZero,
    SearchResult,
    SearchCurrent,
    SearchRestore,
    SearchReturn,
    Exec,
    ExecMethod,
    ExecIndex,
    ExecIndexNumber,
    ExecResult,
    MatchResult,
    MatchValue,
    MatchString,
    Advance,
    AdvanceNumber,
    Advanced,
    GlobalReset,
    AdvanceSet,
    SplitBegin,
    SplitBeginNumber,
    SplitLoop,
    SplitExec,
    SplitResult,
    SplitEnd,
    SplitEndNumber,
    SplitLength,
    SplitLengthNumber,
    SplitCapture,
    SplitCaptureValue,
    Render,
    RenderLength,
    RenderLengthNumber,
    RenderMatch,
    RenderMatchString,
    RenderIndex,
    RenderIndexNumber,
    RenderCapture,
    RenderCaptureValue,
    RenderCaptureString,
    RenderGroups,
    RenderCallback,
    RenderCallbackString,
    Template,
    Named,
    NamedString,
    RenderCommit,
}

#[derive(Debug)]
pub(crate) struct State {
    which: Builtin,
    phase: Phase,
    after_exec: Phase,
    this: Value,
    arg: Value,
    extra: Value,
    rx: Value,
    constructor: Value,
    probe: Value,
    previous: Value,
    result: Value,
    groups: Value,
    value: Value,
    text: JsString,
    flags: JsString,
    template: JsString,
    global: bool,
    unicode: bool,
    functional: bool,
    results: Vec<Value>,
    captures: Vec<Value>,
    out: Value,
    count: usize,
    limit: u64,
    p: usize,
    q: usize,
    end: usize,
    index: usize,
    length: u64,
    capture: u64,
    source_end: usize,
    template_at: usize,
    rendered: Vec<u16>,
    replacement: Vec<u16>,
    work: usize,
    budget: std::sync::Arc<crate::heap::budget::ByteBudget>,
    reservation: crate::heap::budget::ByteReservation,
    split: Option<Box<crate::value::string::StringSplit>>,
    legacy: Option<Box<string_replace::State>>,
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for v in [
            &self.this,
            &self.arg,
            &self.extra,
            &self.rx,
            &self.constructor,
            &self.probe,
            &self.previous,
            &self.result,
            &self.groups,
            &self.value,
            &self.out,
        ] {
            v.trace(visit);
        }
        self.results.trace(visit);
        self.captures.trace(visit);
        if let Some(legacy) = &self.legacy {
            legacy.trace(visit);
        }
    }
}
impl State {
    pub(super) fn new(
        heap: &Heap,
        task: TaskId,
        which: Builtin,
        this: &Value,
        args: &[Value],
    ) -> Result<Self, Fault> {
        use Builtin::*;
        let string = matches!(
            which,
            StringMatch
                | StringMatchAll
                | StringPrototypeSearch
                | StringPrototypeReplace
                | StringPrototypeReplaceAll
                | StringPrototypeSplit
        );
        if string && matches!(this, Value::Null | Value::Undefined) {
            return Err(Fault::TypeError("String method requires a receiver".into()));
        }
        if !string && !matches!(this, Value::Object(_)) {
            return Err(Fault::TypeError("RegExp method requires an object".into()));
        }
        let mut s = Self {
            which,
            phase: if string {
                Phase::Dispatch
            } else {
                Phase::Start
            },
            after_exec: Phase::Return,
            this: this.clone(),
            arg: args.first().cloned().unwrap_or(Value::Undefined),
            extra: args.get(1).cloned().unwrap_or(Value::Undefined),
            rx: this.clone(),
            constructor: Value::Undefined,
            probe: Value::Undefined,
            previous: Value::Undefined,
            result: Value::Undefined,
            groups: Value::Undefined,
            value: Value::Undefined,
            text: "".into(),
            flags: "".into(),
            template: "".into(),
            global: false,
            unicode: false,
            functional: false,
            results: Vec::new(),
            captures: Vec::new(),
            out: Value::Undefined,
            count: 0,
            limit: u32::MAX as u64,
            p: 0,
            q: 0,
            end: 0,
            index: 0,
            length: 0,
            capture: 0,
            source_end: 0,
            template_at: 0,
            rendered: Vec::new(),
            replacement: Vec::new(),
            work: MAX_WORK,
            budget: heap.byte_budget(),
            reservation: Default::default(),
            split: None,
            legacy: None,
        };
        if which == RegExpIteratorNext {
            let data = cursor(heap, task, this, |d| d.clone())?;
            s.rx = data.regexp;
            s.text = data.text;
            s.global = data.global;
            s.unicode = data.unicode;
            s.phase = if data.done {
                Phase::Return
            } else {
                Phase::Exec
            };
            if data.done {
                s.value = s.iterator_result(heap, task, Value::Undefined, true)?;
            }
            s.after_exec = Phase::MatchResult;
        }
        Ok(s)
    }
    fn charge(&mut self, amount: usize) -> Result<(), Fault> {
        let ticket = self
            .budget
            .reserve(amount.checked_mul(256).ok_or_else(|| Fault::HeapLimit)?)
            .ok_or_else(|| Fault::HeapLimit)?;
        self.reservation.merge(ticket);
        self.work = self.work.checked_sub(amount).ok_or_else(|| {
            Fault::RangeError(
                if self.which == Builtin::RegExpReplace {
                    "string replacement work limit exceeded"
                } else {
                    "RegExp symbol work limit exceeded"
                }
                .into(),
            )
        })?;
        Ok(())
    }
    fn go(&mut self, phase: Phase) -> Result<Action, Fault> {
        self.phase = phase;
        Ok(Action::Continue)
    }
    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        key: property::Key,
        next: Phase,
    ) -> Result<Action, Fault> {
        self.phase = next;
        match property::read(heap, task, &base, &key)? {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(base)),
            read => {
                self.value = match read {
                    Some(property::Read::Value(v)) => v,
                    _ => Value::Undefined,
                };
                Ok(Action::Continue)
            }
        }
    }
    fn get(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        name: &str,
        next: Phase,
    ) -> Result<Action, Fault> {
        self.read(
            heap,
            task,
            base,
            property::Key::from_name(&JsString::from(name)),
            next,
        )
    }
    fn set(&mut self, value: Value, next: Phase) -> Result<Action, Fault> {
        self.phase = next;
        Ok(Action::SetProperty {
            this: self.rx.clone(),
            key: object::key("lastIndex"),
            value,
            strict: true,
        })
    }
    fn convert_string(&mut self, value: Value, next: Phase) -> Result<Action, Fault> {
        self.phase = next;
        Ok(Action::ToString(value))
    }
    fn number(&mut self, value: Value, next: Phase) -> Result<Action, Fault> {
        self.phase = next;
        Ok(Action::ToNumberLimited(value, MAX_UNITS))
    }
    fn symbol(&self) -> &'static str {
        use Builtin::*;
        match self.which {
            StringMatch => "match",
            StringMatchAll => "matchAll",
            StringPrototypeSearch => "search",
            StringPrototypeSplit => "split",
            _ => "replace",
        }
    }
    fn exec(&mut self, next: Phase) -> Result<Action, Fault> {
        self.after_exec = next;
        self.go(Phase::Exec)
    }
    fn push(&mut self, heap: &Heap, task: TaskId, value: Value) -> Result<(), Fault> {
        if self.which != Builtin::StringPrototypeSplit {
            self.charge(
                1 + if let Value::String(s) = &value {
                    s.len()
                } else {
                    0
                },
            )?;
        }
        property::append(heap, task, &self.out, Some(value))?;
        self.count += 1;
        Ok(())
    }
    fn iterator_result(
        &self,
        heap: &Heap,
        task: TaskId,
        value: Value,
        done: bool,
    ) -> Result<Value, Fault> {
        let result = builtin::new_object(heap, task)?;
        for (key, v) in [("value", value), ("done", Value::Bool(done))] {
            property::prepare_put(heap, task, &result, &object::key(key), v, true)?;
        }
        Ok(result)
    }
    fn finish_iterator(&mut self, heap: &Heap, task: TaskId, done: bool) -> Result<Action, Fault> {
        if done || !self.global {
            cursor(heap, task, &self.this, |d| {
                d.done = true;
                d.regexp = Value::Undefined;
                d.text = "".into();
            })?;
        }
        self.iterator_result(
            heap,
            task,
            if done {
                Value::Undefined
            } else {
                self.result.clone()
            },
            done,
        )
        .map(Action::Return)
    }
    fn advance(&mut self) -> Result<Action, Fault> {
        self.go(Phase::Advance)
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        if let Some(legacy) = &mut self.legacy {
            return legacy.step(heap, task, input);
        }
        match input {
            Input::Start => {}
            Input::CallResult(v) => self.value = v,
            _ => return Err(Fault::Internal("RegExp symbol unexpected dependency")),
        }
        use Phase::*;
        match self.phase {
            Dispatch | IsRegExp | CheckFlags | CheckFlagsString | Method | Invoke | Return
            | FallbackText | FallbackCreated | FallbackMethod | SplitLimit | SplitLimitNumber
            | SplitSeparator | SplitSeparatorString | StringSplit => self.step_dispatch(heap, task),
            Start | Text | Replacement | Flags | FlagsString | Constructor | Species
            | SpeciesValue | SpeciesCheck | SpeciesFlags | SpeciesFlagsString | Created
            | CopyIndex | CopyIndexNumber | IteratorCreate | SearchIndex | SearchZero
            | SearchResult | SearchCurrent | SearchRestore | SearchReturn => {
                self.step_initialize(heap, task)
            }
            Exec | ExecMethod | ExecIndex | ExecIndexNumber | ExecResult | MatchResult
            | MatchValue | MatchString | Advance | AdvanceNumber | Advanced | GlobalReset
            | AdvanceSet => self.step_exec(heap, task),
            SplitBegin | SplitBeginNumber | SplitLoop | SplitExec | SplitResult | SplitEnd
            | SplitEndNumber | SplitLength | SplitLengthNumber | SplitCapture
            | SplitCaptureValue => self.step_split(heap, task),
            Render | RenderLength | RenderLengthNumber | RenderMatch | RenderMatchString
            | RenderIndex | RenderIndexNumber | RenderCapture | RenderCaptureValue
            | RenderCaptureString | RenderGroups | RenderCallback | RenderCallbackString
            | Template | Named | NamedString | RenderCommit => self.step_render(heap, task),
        }
    }
    // Isolate each protocol's temporaries: keeping every phase in one debug
    // frame overflows a 64 KiB VM worker when the matcher/property code runs.
    #[inline(never)]
    fn step_dispatch(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let v = self.value.clone();
        use Builtin::*;
        use Phase::*;
        match self.phase {
            Dispatch => {
                if matches!(self.which, StringMatchAll | StringPrototypeReplaceAll)
                    && matches!(self.arg, Value::Object(_))
                {
                    self.read(
                        heap,
                        task,
                        self.arg.clone(),
                        property::Key::Symbol(symbol::well_known("match")),
                        IsRegExp,
                    )
                } else {
                    self.go(Method)
                }
            }
            IsRegExp => {
                let is_regexp = if matches!(v, Value::Undefined) {
                    object::data(heap, task, &self.arg)?.is_some()
                } else {
                    v.truthy()
                };
                if is_regexp {
                    self.get(heap, task, self.arg.clone(), "flags", CheckFlags)
                } else {
                    self.go(Method)
                }
            }
            CheckFlags => {
                if matches!(v, Value::Null | Value::Undefined) {
                    return Err(Fault::TypeError("RegExp flags require a value".into()));
                }
                self.convert_string(v, CheckFlagsString)
            }
            CheckFlagsString => {
                if !string(v)?.code_units().contains(&(b'g' as u16)) {
                    return Err(Fault::TypeError(
                        "matchAll/replaceAll requires a global RegExp".into(),
                    ));
                }
                self.go(Method)
            }
            Method => {
                if matches!(self.arg, Value::Null | Value::Undefined) {
                    self.value = Value::Undefined;
                    self.go(Invoke)
                } else {
                    self.read(
                        heap,
                        task,
                        self.arg.clone(),
                        property::Key::Symbol(symbol::well_known(self.symbol())),
                        Invoke,
                    )
                }
            }
            Invoke => {
                if !matches!(v, Value::Null | Value::Undefined) {
                    dispatch::callee(heap, &v)?;
                    self.phase = Return;
                    let mut args = vec![self.this.clone()];
                    if matches!(
                        self.which,
                        StringPrototypeSplit | StringPrototypeReplace | StringPrototypeReplaceAll
                    ) {
                        args.push(self.extra.clone());
                    }
                    return Ok(Action::Call {
                        callee: v,
                        this: self.arg.clone(),
                        args,
                    });
                }
                if matches!(
                    self.which,
                    StringPrototypeReplace | StringPrototypeReplaceAll
                ) {
                    self.legacy = Some(Box::new(string_replace::State::new(
                        heap,
                        self.which,
                        &self.this,
                        &[self.arg.clone(), self.extra.clone()],
                    )?));
                    return Ok(Action::Continue);
                }
                self.convert_string(self.this.clone(), FallbackText)
            }
            Return => Ok(Action::Return(v)),
            FallbackText => {
                self.text = if self.which == StringPrototypeSplit {
                    coerced_string(v)?
                } else {
                    string(v)?
                };
                if self.which == StringPrototypeSplit {
                    return self.go(SplitLimit);
                }
                self.phase = FallbackCreated;
                let ctor = crate::realm::intrinsics(heap)
                    .ok_or_else(|| Fault::OutOfMemory("RegExp realm"))?
                    .global(crate::realm::Global::RegExp);
                Ok(Action::Construct {
                    callee: ctor,
                    args: vec![
                        self.arg.clone(),
                        if self.which == StringMatchAll {
                            Value::String(JsString::fixed("g"))
                        } else {
                            Value::Undefined
                        },
                    ],
                })
            }
            FallbackCreated => {
                self.rx = v;
                self.read(
                    heap,
                    task,
                    self.rx.clone(),
                    property::Key::Symbol(symbol::well_known(self.symbol())),
                    FallbackMethod,
                )
            }
            FallbackMethod => {
                self.phase = Return;
                Ok(Action::Call {
                    callee: v,
                    this: self.rx.clone(),
                    args: vec![Value::String(self.text.clone())],
                })
            }
            SplitLimit => {
                if matches!(self.extra, Value::Undefined) {
                    self.go(SplitSeparator)
                } else {
                    self.phase = SplitLimitNumber;
                    Ok(Action::ToNumber(self.extra.clone()))
                }
            }
            SplitLimitNumber => {
                self.limit = dispatch::to_int32(dispatch::to_number(&v)?) as u32 as u64;
                self.go(SplitSeparator)
            }
            SplitSeparator => {
                self.out = property::new_array(heap, task)?;
                // ToString(separator) precedes the zero-limit return.
                if matches!(self.arg, Value::Undefined) {
                    self.split = Some(Box::new(crate::value::string::StringSplit::new_accounted(
                        heap,
                        self.text.clone(),
                        None,
                    )?));
                    self.go(StringSplit)
                } else {
                    self.convert_string(self.arg.clone(), SplitSeparatorString)
                }
            }
            SplitSeparatorString => {
                self.split = Some(Box::new(crate::value::string::StringSplit::new_accounted(
                    heap,
                    self.text.clone(),
                    Some(coerced_string(v)?),
                )?));
                self.go(StringSplit)
            }
            StringSplit => {
                if self.count as u64 == self.limit {
                    return Ok(Action::Return(self.out.clone()));
                }
                match self
                    .split
                    .as_mut()
                    .unwrap()
                    .advance()
                    .ok_or_else(|| Fault::OutOfMemory("split string allocation failed"))?
                {
                    crate::value::string::SplitStep::Part(s) => {
                        self.push(heap, task, Value::String(s))?
                    }
                    crate::value::string::SplitStep::Done => {
                        return Ok(Action::Return(self.out.clone()));
                    }
                    crate::value::string::SplitStep::Continue => {}
                }
                Ok(Action::Continue)
            }
            _ => Err(Fault::Internal("RegExp dispatch phase")),
        }
    }
    #[inline(never)]
    fn step_initialize(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let v = self.value.clone();
        use Builtin::*;
        use Phase::*;
        match self.phase {
            Start => self.convert_string(self.arg.clone(), Text),
            Text => {
                self.text = string(v)?;
                match self.which {
                    RegExpSearch => self.get(heap, task, self.rx.clone(), "lastIndex", SearchIndex),
                    RegExpMatchAll | RegExpSplit => {
                        self.get(heap, task, self.rx.clone(), "constructor", Constructor)
                    }
                    RegExpReplace => {
                        self.functional = dispatch::callee(heap, &self.extra).is_ok();
                        if self.functional {
                            self.go(Flags)
                        } else {
                            self.convert_string(self.extra.clone(), Replacement)
                        }
                    }
                    _ => self.go(Flags),
                }
            }
            Replacement => {
                self.template = string(v)?;
                self.go(Flags)
            }
            Flags => self.get(heap, task, self.rx.clone(), "flags", FlagsString),
            FlagsString => {
                // Conversion has its own phase, so even object-valued flags suspend.
                self.convert_string(v, SpeciesFlagsString)
            }
            Constructor => {
                let default = crate::realm::intrinsics(heap)
                    .ok_or_else(|| Fault::OutOfMemory("RegExp realm"))?
                    .global(crate::realm::Global::RegExp);
                self.constructor = default;
                if matches!(v, Value::Undefined) {
                    self.go(SpeciesFlags)
                } else if matches!(v, Value::Object(_)) {
                    self.read(
                        heap,
                        task,
                        v,
                        property::Key::Symbol(symbol::well_known("species")),
                        SpeciesValue,
                    )
                } else {
                    Err(Fault::TypeError(
                        "RegExp constructor must be an object".into(),
                    ))
                }
            }
            SpeciesValue => {
                if !matches!(v, Value::Null | Value::Undefined) {
                    self.constructor = v;
                }
                self.probe = self.constructor.clone();
                self.go(SpeciesCheck)
            }
            SpeciesCheck => {
                self.charge(1)?;
                if let Ok(dispatch::Callee::Bound(bound)) = dispatch::callee(heap, &self.probe) {
                    self.probe = bound.callee;
                    return Ok(Action::Continue);
                }
                if !dispatch::is_constructor(heap, &self.probe, 1)? {
                    return Err(Fault::TypeError(
                        "RegExp species must be a constructor".into(),
                    ));
                }
                self.go(SpeciesFlags)
            }
            SpeciesFlags => self.get(heap, task, self.rx.clone(), "flags", Species),
            Species => self.convert_string(v, SpeciesFlagsString),
            SpeciesFlagsString => {
                self.flags = string(v)?;
                self.global = self.flags.code_units().contains(&(b'g' as u16));
                self.unicode = self
                    .flags
                    .code_units()
                    .iter()
                    .any(|u| *u == b'u' as u16 || *u == b'v' as u16);
                if matches!(self.which, RegExpSplit | RegExpMatchAll) {
                    let flags = if self.which == RegExpSplit
                        && !self.flags.code_units().contains(&(b'y' as u16))
                    {
                        self.flags.concat_accounted(heap, &"y".into())?
                    } else {
                        self.flags.clone()
                    };
                    self.phase = Created;
                    Ok(Action::Construct {
                        callee: self.constructor.clone(),
                        args: vec![self.rx.clone(), Value::String(flags)],
                    })
                } else {
                    if self.which == RegExpMatch {
                        self.out = property::new_array(heap, task)?;
                    }
                    if self.global {
                        self.set(Value::Number(0.0), GlobalReset)
                    } else {
                        self.exec(MatchResult)
                    }
                }
            }
            Created => {
                self.rx = v;
                if self.which == RegExpSplit {
                    self.out = property::new_array(heap, task)?;
                    self.go(SplitBegin)
                } else {
                    self.get(heap, task, self.this.clone(), "lastIndex", CopyIndex)
                }
            }
            CopyIndex => self.number(v, CopyIndexNumber),
            CopyIndexNumber => {
                self.set(Value::Number(super::to_length(&v)? as f64), IteratorCreate)
            }
            IteratorCreate => {
                let proto = crate::realm::intrinsics(heap)
                    .ok_or_else(|| Fault::OutOfMemory("RegExp realm"))?
                    .regexp_iterator_prototype;
                let o = OrdinaryObject::with_prototype(heap, task, proto).with_regexp_iterator(
                    IteratorData {
                        regexp: self.rx.clone(),
                        text: self.text.clone(),
                        global: self.global,
                        unicode: self.unicode,
                        done: false,
                    },
                );
                heap.alloc(HeapObject::Plain(o))
                    .map(Value::Object)
                    .map(Action::Return)
                    .ok_or_else(|| Fault::OutOfMemory("RegExp iterator"))
            }
            SearchIndex => {
                self.previous = v;
                if crate::atom::same_value(&self.previous, &Value::Number(0.0)) {
                    self.exec(SearchResult)
                } else {
                    self.set(Value::Number(0.0), SearchZero)
                }
            }
            SearchZero => self.exec(SearchResult),
            SearchResult => {
                self.result = v;
                self.get(heap, task, self.rx.clone(), "lastIndex", SearchCurrent)
            }
            SearchCurrent => {
                if crate::atom::same_value(&self.previous, &v) {
                    self.go(SearchReturn)
                } else {
                    self.set(self.previous.clone(), SearchRestore)
                }
            }
            SearchRestore => self.go(SearchReturn),
            SearchReturn => {
                if matches!(self.result, Value::Null) {
                    Ok(Action::Return(Value::Number(-1.0)))
                } else {
                    self.get(heap, task, self.result.clone(), "index", Return)
                }
            }
            _ => Err(Fault::Internal("RegExp initialization phase")),
        }
    }
    #[inline(never)]
    fn step_exec(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let v = self.value.clone();
        use Builtin::*;
        use Phase::*;
        match self.phase {
            Exec => {
                self.charge(1)?;
                self.get(heap, task, self.rx.clone(), "exec", ExecMethod)
            }
            ExecMethod => {
                // Callee classification unwraps Proxies; the fast path must
                // recognize the value itself so wrapper call checks still run.
                let intrinsic = if let Value::Object(id) = &v {
                    heap.read(
                        *id,
                        |o| matches!(o, HeapObject::Builtin(f) if f.which == RegExpExec),
                    )
                    .ok_or_else(|| Fault::Internal("exec callee reclaimed"))?
                } else {
                    false
                };
                if intrinsic {
                    object::receiver(heap, task, &self.rx)?;
                    self.get(heap, task, self.rx.clone(), "lastIndex", ExecIndex)
                } else if dispatch::callee(heap, &v).is_ok() {
                    self.phase = ExecResult;
                    Ok(Action::Call {
                        callee: v,
                        this: self.rx.clone(),
                        args: vec![Value::String(self.text.clone())],
                    })
                } else {
                    object::receiver(heap, task, &self.rx)?;
                    self.get(heap, task, self.rx.clone(), "lastIndex", ExecIndex)
                }
            }
            ExecIndex => self.number(v, ExecIndexNumber),
            ExecIndexNumber => {
                self.value = object::exec_with_capture_budget(
                    heap,
                    task,
                    &self.rx,
                    &self.text,
                    super::to_length(&v)?,
                    if self.which == RegExpReplace {
                        Some(&mut self.work)
                    } else {
                        None
                    },
                )?;
                self.go(ExecResult)
            }
            ExecResult => {
                if !matches!(v, Value::Null | Value::Object(_)) {
                    return Err(Fault::TypeError(
                        "RegExp exec must return an object or null".into(),
                    ));
                }
                self.go(self.after_exec)
            }
            MatchResult => {
                if matches!(v, Value::Null) {
                    return match self.which {
                        RegExpReplace => self.go(Render),
                        RegExpIteratorNext => self.finish_iterator(heap, task, true),
                        _ => Ok(Action::Return(if self.count == 0 {
                            Value::Null
                        } else {
                            self.out.clone()
                        })),
                    };
                }
                self.result = v;
                if self.which == RegExpReplace {
                    self.charge(1)?;
                    self.results.push(self.result.clone());
                    if !self.global {
                        return self.go(Render);
                    }
                } else if !self.global {
                    return if self.which == RegExpIteratorNext {
                        self.finish_iterator(heap, task, false)
                    } else {
                        Ok(Action::Return(self.result.clone()))
                    };
                }
                self.get(heap, task, self.result.clone(), "0", MatchValue)
            }
            MatchValue => self.convert_string(v, MatchString),
            MatchString => {
                let s = string(v)?;
                if self.which == RegExpMatch {
                    self.push(heap, task, Value::String(s.clone()))?;
                }
                if s.is_empty() {
                    self.advance()
                } else if self.which == RegExpIteratorNext {
                    self.finish_iterator(heap, task, false)
                } else {
                    self.exec(MatchResult)
                }
            }
            Advance => self.get(heap, task, self.rx.clone(), "lastIndex", AdvanceNumber),
            AdvanceNumber => self.number(v, Advanced),
            Advanced => {
                let i = super::to_length(&v)? as usize;
                let next = matcher::advance_string_index(self.text.code_units(), i, self.unicode);
                self.set(Value::Number(next as f64), AdvanceSet)
            }
            GlobalReset => self.exec(MatchResult),
            AdvanceSet => {
                if self.which == RegExpIteratorNext {
                    self.finish_iterator(heap, task, false)
                } else {
                    self.exec(MatchResult)
                }
            }
            _ => Err(Fault::Internal("RegExp exec phase")),
        }
    }
    #[inline(never)]
    fn step_split(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let v = self.value.clone();
        use Phase::*;
        match self.phase {
            SplitBegin => {
                if matches!(self.extra, Value::Undefined) {
                    self.go(SplitLoop)
                } else {
                    self.number(self.extra.clone(), SplitBeginNumber)
                }
            }
            SplitBeginNumber => {
                self.limit = dispatch::to_int32(dispatch::to_number(&v)?) as u32 as u64;
                self.go(SplitLoop)
            }
            SplitLoop => {
                if self.count as u64 == self.limit {
                    return Ok(Action::Return(self.out.clone()));
                }
                if self.text.is_empty() {
                    return self.exec(SplitResult);
                }
                if self.q >= self.text.len() {
                    self.push(
                        heap,
                        task,
                        Value::String(slice(heap, &self.text, self.p, self.text.len())?),
                    )?;
                    return Ok(Action::Return(self.out.clone()));
                }
                self.set(Value::Number(self.q as f64), SplitExec)
            }
            SplitExec => self.exec(SplitResult),
            SplitResult => {
                if self.text.is_empty() {
                    if matches!(v, Value::Null) {
                        self.push(heap, task, Value::String(self.text.clone()))?;
                    }
                    return Ok(Action::Return(self.out.clone()));
                }
                if matches!(v, Value::Null) {
                    self.q =
                        matcher::advance_string_index(self.text.code_units(), self.q, self.unicode);
                    return self.go(SplitLoop);
                }
                self.result = v;
                self.get(heap, task, self.rx.clone(), "lastIndex", SplitEnd)
            }
            SplitEnd => self.number(v, SplitEndNumber),
            SplitEndNumber => {
                self.end = (super::to_length(&v)? as usize).min(self.text.len());
                if self.end == self.p {
                    self.q =
                        matcher::advance_string_index(self.text.code_units(), self.q, self.unicode);
                    self.go(SplitLoop)
                } else {
                    self.push(
                        heap,
                        task,
                        Value::String(slice(heap, &self.text, self.p, self.q)?),
                    )?;
                    self.p = self.end;
                    if self.count as u64 == self.limit {
                        return Ok(Action::Return(self.out.clone()));
                    }
                    self.get(heap, task, self.result.clone(), "length", SplitLength)
                }
            }
            SplitLength => self.number(v, SplitLengthNumber),
            SplitLengthNumber => {
                self.length = super::to_length(&v)?;
                self.capture = 1;
                self.go(SplitCapture)
            }
            SplitCapture => {
                if self.capture >= self.length {
                    self.q = self.p;
                    return self.go(SplitLoop);
                }
                self.get(
                    heap,
                    task,
                    self.result.clone(),
                    &self.capture.to_string(),
                    SplitCaptureValue,
                )
            }
            SplitCaptureValue => {
                self.push(heap, task, v)?;
                self.capture += 1;
                if self.count as u64 == self.limit {
                    Ok(Action::Return(self.out.clone()))
                } else {
                    self.go(SplitCapture)
                }
            }
            _ => Err(Fault::Internal("RegExp split phase")),
        }
    }
    #[inline(never)]
    fn step_render(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let v = self.value.clone();
        use Phase::*;
        match self.phase {
            Render => {
                let Some(result) = self.results.get(self.index).cloned() else {
                    let suffix = self.source_end.min(self.text.len());
                    self.charge(self.text.len() - suffix)?;
                    self.rendered
                        .extend_from_slice(&self.text.code_units()[suffix..]);
                    return Ok(Action::Return(Value::String(
                        JsString::from_owned_code_units(std::mem::take(&mut self.rendered))
                            .ok_or_else(|| Fault::StringTooLong)?
                            .with_reservation(Some(std::sync::Arc::new(std::mem::take(
                                &mut self.reservation,
                            )))),
                    )));
                };
                self.result = result;
                self.captures.clear();
                self.replacement.clear();
                self.template_at = 0;
                self.get(heap, task, self.result.clone(), "length", RenderLength)
            }
            RenderLength => self.number(v, RenderLengthNumber),
            RenderLengthNumber => {
                self.length = super::to_length(&v)?.saturating_sub(1);
                self.charge(self.length as usize)?;
                self.get(heap, task, self.result.clone(), "0", RenderMatch)
            }
            RenderMatch => self.convert_string(v, RenderMatchString),
            RenderMatchString => {
                let s = string(v)?;
                self.charge(s.len())?;
                self.captures.push(Value::String(s));
                self.get(heap, task, self.result.clone(), "index", RenderIndex)
            }
            RenderIndex => self.number(v, RenderIndexNumber),
            RenderIndexNumber => {
                let n = dispatch::to_number(&v)?;
                self.p = if n.is_nan() {
                    0
                } else {
                    n.trunc().clamp(0.0, self.text.len() as f64) as usize
                };
                self.capture = 1;
                self.go(RenderCapture)
            }
            RenderCapture => {
                if self.capture > self.length {
                    return self.get(heap, task, self.result.clone(), "groups", RenderGroups);
                }
                self.get(
                    heap,
                    task,
                    self.result.clone(),
                    &self.capture.to_string(),
                    RenderCaptureValue,
                )
            }
            RenderCaptureValue => {
                if matches!(v, Value::Undefined) {
                    self.captures.push(v);
                    self.capture += 1;
                    self.go(RenderCapture)
                } else {
                    self.convert_string(v, RenderCaptureString)
                }
            }
            RenderCaptureString => {
                let s = string(v)?;
                self.charge(s.len())?;
                self.captures.push(Value::String(s));
                self.capture += 1;
                self.go(RenderCapture)
            }
            RenderGroups => {
                self.groups = v;
                if self.functional {
                    let mut args = self.captures.clone();
                    args.push(Value::Number(self.p as f64));
                    args.push(Value::String(self.text.clone()));
                    if !matches!(self.groups, Value::Undefined) {
                        args.push(self.groups.clone());
                    }
                    self.phase = RenderCallback;
                    Ok(Action::Call {
                        callee: self.extra.clone(),
                        this: Value::Undefined,
                        args,
                    })
                } else {
                    if matches!(self.groups, Value::Null) {
                        return Err(Fault::TypeError("capture groups cannot be null".into()));
                    }
                    self.go(Template)
                }
            }
            RenderCallback => self.convert_string(v, RenderCallbackString),
            RenderCallbackString => {
                let s = string(v)?;
                self.append_replacement(s.code_units())?;
                self.go(RenderCommit)
            }
            Template => self.template_step(heap, task),
            Named => {
                if matches!(v, Value::Undefined) {
                    self.go(Template)
                } else {
                    self.convert_string(v, NamedString)
                }
            }
            NamedString => {
                let s = string(v)?;
                self.append_replacement(s.code_units())?;
                self.go(Template)
            }
            RenderCommit => {
                if self.p >= self.source_end {
                    let n = self.p - self.source_end + self.replacement.len();
                    self.charge(n)?;
                    self.rendered
                        .extend_from_slice(&self.text.code_units()[self.source_end..self.p]);
                    self.rendered.extend_from_slice(&self.replacement);
                    let Value::String(m) = &self.captures[0] else {
                        unreachable!()
                    };
                    self.source_end = self.p + m.len();
                }
                self.index += 1;
                self.go(Render)
            }
            _ => Err(Fault::Internal("RegExp render phase")),
        }
    }
    fn append_replacement(&mut self, units: &[u16]) -> Result<(), Fault> {
        self.charge(units.len())?;
        self.replacement.extend_from_slice(units);
        Ok(())
    }
    fn template_step(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let units = self.template.code_units();
        let i = self.template_at;
        if i == units.len() {
            return self.go(Phase::RenderCommit);
        }
        self.charge(1)?;
        let units = self.template.code_units();
        let c = units[i];
        if c != 36 || i + 1 == units.len() {
            self.template_at += 1;
            self.append_replacement(&[c])?;
            return Ok(Action::Continue);
        }
        let token = units[i + 1];
        let Value::String(whole) = &self.captures[0] else {
            unreachable!()
        };
        let end = (self.p + whole.len()).min(self.text.len());
        let mut consumed = 2;
        let replacement = match token {
            36 => Some(Value::String("$".into())),
            38 => Some(self.captures[0].clone()),
            96 => Some(Value::String(slice(heap, &self.text, 0, self.p)?)),
            39 => Some(Value::String(slice(
                heap,
                &self.text,
                end,
                self.text.len(),
            )?)),
            48..=57 => {
                let mut n = (token - 48) as usize;
                if let Some(&digit @ 48..=57) = units.get(i + 2) {
                    let double = n * 10 + (digit - 48) as usize;
                    if double > 0 && double < self.captures.len() {
                        n = double;
                        consumed = 3;
                    }
                }
                if n > 0 && n < self.captures.len() {
                    Some(self.captures[n].clone())
                } else {
                    consumed = 1;
                    Some(Value::String("$".into()))
                }
            }
            60 if !matches!(self.groups, Value::Undefined) => {
                let offset = units[i + 2..].iter().position(|u| *u == 62);
                let scanned = offset.map_or(units.len() - i - 2, |n| n + 1);
                self.charge(scanned)?;
                if let Some(offset) = offset {
                    let end = i + 2 + offset;
                    let name = JsString::from_code_units(&self.template.code_units()[i + 2..end])
                        .ok_or_else(|| Fault::StringTooLong)?;
                    self.template_at = end + 1;
                    return self.read(
                        heap,
                        task,
                        self.groups.clone(),
                        property::Key::from_name(&name),
                        Phase::Named,
                    );
                }
                consumed = 1;
                Some(Value::String("$".into()))
            }
            _ => {
                consumed = 1;
                Some(Value::String("$".into()))
            }
        };
        self.template_at += consumed;
        if let Some(Value::String(s)) = replacement {
            self.append_replacement(s.code_units())?;
        }
        Ok(Action::Continue)
    }
}
fn string(v: Value) -> Result<JsString, Fault> {
    let s = coerced_string(v)?;
    if s.len() > MAX_UNITS {
        return Err(Fault::RangeError(
            "RegExp symbol argument limit of 65536 code units exceeded".into(),
        ));
    }
    Ok(s)
}
fn coerced_string(v: Value) -> Result<JsString, Fault> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err(Fault::Internal("RegExp symbol string conversion missing")),
    }
}
fn slice(heap: &Heap, text: &JsString, start: usize, end: usize) -> Result<JsString, Fault> {
    let ticket = JsString::reserve(heap, end - start, 1)?;
    JsString::from_code_units(&text.code_units()[start..end])
        .map(|text| text.with_reservation(ticket))
        .ok_or_else(|| Fault::StringTooLong)
}
