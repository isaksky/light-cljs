//! Brand-dispatched replacement. Scans/match collection resume between native
//! steps; matcher calls are bounded leaves. Coercions and replacers use VM frames.
use std::sync::Arc;

use super::{Action, Input};
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::regexp::{matcher, object};
use crate::value::string::StringSearch;
use crate::value::{JsString, Value};
use crate::vm::{builtin::Builtin, dispatch, dispatch::Fault, property};

// Check before flattening a rope. Copying/substitution/capture storage has a
// separate aggregate allowance, shared across all native steps and callbacks.
const MAX_ARGUMENT_UNITS: usize = 65_536;
const MAX_REPLACE_WORK: usize = 1_000_000;

#[derive(Debug)]
enum Phase {
    Start,
    Text,
    Search,
    Replacement,
    LastIndex,
    LastNumber,
    Collect,
    Render,
    Callback,
    CallbackString,
}

#[derive(Debug)]
pub(super) struct State {
    this: Value,
    search: Value,
    replacement: Value,
    regexp: Option<Arc<object::Compiled>>,
    all: bool,
    functional: bool,
    phase: Phase,
    text: JsString,
    needle: JsString,
    template: JsString,
    scan: Option<StringSearch>,
    next: usize,
    collected: bool,
    results: Vec<matcher::Captures>,
    current: usize,
    source_end: usize,
    out: Vec<u16>,
    work: usize,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.this.trace(visit);
        self.search.trace(visit);
        self.replacement.trace(visit);
        // All other fields are immutable strings/programs or Rust buffers,
        // without heap edges. Callback arguments belong to ordinary VM frames.
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
        if matches!(this, Value::Null | Value::Undefined) {
            return Err(Fault::TypeError(
                "String replacement requires a receiver".into(),
            ));
        }
        let search = args.first().cloned().unwrap_or(Value::Undefined);
        let regexp = object::data(heap, task, &search)?;
        let all = which == Builtin::StringPrototypeReplaceAll;
        if all && regexp.as_ref().is_some_and(|r| !r.flags.global) {
            return Err(Fault::TypeError(
                "replaceAll requires a global RegExp".into(),
            ));
        }
        Ok(Self {
            this: this.clone(),
            search,
            replacement: args.get(1).cloned().unwrap_or(Value::Undefined),
            regexp,
            all,
            functional: false,
            phase: Phase::Start,
            text: JsString::from(""),
            needle: JsString::from(""),
            template: JsString::from(""),
            scan: None,
            next: 0,
            collected: false,
            results: Vec::new(),
            current: 0,
            source_end: 0,
            out: Vec::new(),
            work: MAX_REPLACE_WORK,
        })
    }

    fn replacement_action(&mut self, heap: &Heap) -> Action {
        self.functional = dispatch::callee(heap, &self.replacement).is_ok();
        self.phase = Phase::Replacement;
        if self.functional {
            Action::Continue
        } else {
            Action::ToString(self.replacement.clone())
        }
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        let v = match input {
            Input::Start => Value::Undefined,
            Input::CallResult(v) => v,
            _ => return Err(Fault::Internal("replacement unexpected dependency")),
        };
        match self.phase {
            Phase::Start => {
                self.phase = Phase::Text;
                Ok(Action::ToString(self.this.clone()))
            }
            Phase::Text => {
                self.text = string(v)?;
                if self.regexp.is_some() {
                    Ok(self.replacement_action(heap))
                } else {
                    self.phase = Phase::Search;
                    Ok(Action::ToString(self.search.clone()))
                }
            }
            Phase::Search => {
                self.needle = string(v)?;
                Ok(self.replacement_action(heap))
            }
            Phase::Replacement => {
                if !self.functional {
                    self.template = string(v)?;
                }
                if let Some(re) = &self.regexp {
                    if re.flags.global {
                        set_last_index(heap, task, &self.search, 0)?;
                    }
                    self.phase = Phase::LastIndex;
                } else {
                    self.scan = Some(StringSearch::new(
                        self.text.clone(),
                        self.needle.clone(),
                        0,
                        false,
                    ));
                    self.phase = Phase::Collect;
                }
                Ok(Action::Continue)
            }
            Phase::LastIndex => {
                // Branded regexps have a non-configurable own data property.
                // ToLength can still invoke JavaScript when its value is an object.
                let last = property::get(heap, task, &self.search, &object::key("lastIndex"))?;
                self.phase = Phase::LastNumber;
                Ok(Action::ToNumberLimited(last, MAX_ARGUMENT_UNITS))
            }
            Phase::LastNumber => {
                self.next = usize::try_from(super::to_length(&v)?).unwrap_or(usize::MAX);
                self.phase = Phase::Collect;
                Ok(Action::Continue)
            }
            Phase::Collect => self.collect(heap, task),
            Phase::Render => {
                let Some(m) = self.results.get(self.current) else {
                    if self.results.is_empty() {
                        return Ok(Action::Return(Value::String(self.text.clone())));
                    }
                    append(
                        &mut self.out,
                        &mut self.work,
                        &self.text.code_units()[self.source_end..],
                    )?;
                    let text = JsString::from_owned_code_units(std::mem::take(&mut self.out))
                        .ok_or_else(limit)?;
                    return Ok(Action::Return(Value::String(text)));
                };
                let span = m.groups[0].as_ref().expect("whole match");
                append(
                    &mut self.out,
                    &mut self.work,
                    &self.text.code_units()[self.source_end..span.start],
                )?;
                self.source_end = span.end;
                if self.functional {
                    let args = self.callback_args(heap, task)?;
                    self.phase = Phase::Callback;
                    Ok(Action::Call {
                        callee: self.replacement.clone(),
                        this: Value::Undefined,
                        args,
                    })
                } else {
                    substitute(
                        &self.text,
                        &self.template,
                        m,
                        self.regexp.as_ref().map(|re| re.names.as_slice()),
                        &mut self.out,
                        &mut self.work,
                    )?;
                    self.current += 1;
                    Ok(Action::Continue)
                }
            }
            Phase::Callback => {
                self.phase = Phase::CallbackString;
                Ok(Action::ToString(v))
            }
            Phase::CallbackString => {
                let text = string(v)?;
                append(&mut self.out, &mut self.work, text.code_units())?;
                self.current += 1;
                self.phase = Phase::Render;
                Ok(Action::Continue)
            }
        }
    }

    fn collect(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        if self.collected {
            self.phase = Phase::Render;
            return Ok(Action::Continue);
        }
        let found = if let Some(re) = &self.regexp {
            let stateful = re.flags.global || re.flags.sticky;
            let start = if stateful { self.next } else { 0 };
            let found =
                matcher::exec(&re.program, self.text.code_units(), start).map_err(|e| match e {
                    matcher::MatchError::LimitExceeded => {
                        Fault::RangeError("regular expression step limit exceeded".into())
                    }
                    _ => Fault::SyntaxError(e.to_string()),
                })?;
            if stateful {
                let end = found
                    .as_ref()
                    .map_or(0, |m| m.groups[0].as_ref().unwrap().end);
                set_last_index(heap, task, &self.search, end)?;
                self.next = end;
                if re.flags.global
                    && let Some(m) = &found
                    && m.groups[0].as_ref().unwrap().is_empty()
                {
                    self.next = matcher::advance_string_index(
                        self.text.code_units(),
                        end,
                        re.flags.unicode || re.flags.unicode_sets,
                    );
                    set_last_index(heap, task, &self.search, self.next)?;
                }
            }
            self.collected = found.is_none() || !re.flags.global;
            found
        } else {
            if self.next > self.text.len() {
                self.collected = true;
                return Ok(Action::Continue);
            }
            let position = if self.needle.is_empty() {
                self.next as isize
            } else {
                let Some(position) = self.scan.as_mut().unwrap().advance(256) else {
                    return Ok(Action::Continue);
                };
                position
            };
            if position < 0 {
                self.collected = true;
                None
            } else {
                let start = position as usize;
                let end = start + self.needle.len();
                self.next = end + usize::from(self.needle.is_empty());
                self.collected = !self.all;
                self.scan.as_mut().unwrap().restart(self.next);
                Some(matcher::Captures {
                    index: start,
                    groups: vec![Some(start..end)],
                })
            }
        };
        if let Some(m) = found {
            charge(&mut self.work, m.groups.len() + 1)?;
            self.results.push(m);
        }
        Ok(Action::Continue)
    }

    fn callback_args(&mut self, heap: &Heap, task: TaskId) -> Result<Vec<Value>, Fault> {
        let m = &self.results[self.current];
        let mut args = Vec::with_capacity(m.groups.len() + 3);
        for span in &m.groups {
            args.push(if let Some(span) = span {
                charge(&mut self.work, span.len())?;
                Value::String(
                    JsString::from_code_units(&self.text.code_units()[span.clone()]).unwrap(),
                )
            } else {
                Value::Undefined
            });
        }
        args.push(Value::Number(m.index as f64));
        args.push(Value::String(self.text.clone()));
        if let Some(re) = &self.regexp
            && re.names.iter().any(Option::is_some)
        {
            let mut groups = OrdinaryObject::new(Some(task));
            for (i, name) in re.names.iter().enumerate() {
                if let Some(name) = name {
                    charge(&mut self.work, name.len() + 1)?;
                    let key = heap
                        .keys()
                        .intern(name)
                        .ok_or(Fault::OutOfMemory("capture name"))?;
                    let value = args[i + 1].clone();
                    if !matches!(value, Value::Undefined) || groups.get_own(key).is_none() {
                        groups.define_with(key, value, PropertyFlags::DATA);
                    }
                }
            }
            args.push(Value::Object(
                heap.alloc(HeapObject::Plain(groups))
                    .ok_or(Fault::OutOfMemory("replacement groups"))?,
            ));
        }
        Ok(args)
    }
}

fn limit() -> Fault {
    Fault::RangeError("string replacement work limit exceeded".into())
}

fn charge(work: &mut usize, units: usize) -> Result<(), Fault> {
    *work = work.checked_sub(units).ok_or_else(limit)?;
    Ok(())
}

fn string(v: Value) -> Result<JsString, Fault> {
    let Value::String(text) = v else {
        return Err(Fault::Internal("replacement string conversion missing"));
    };
    if text.len() > MAX_ARGUMENT_UNITS {
        return Err(Fault::RangeError(
            "string replacement argument limit of 65536 code units exceeded".into(),
        ));
    }
    Ok(text)
}

fn append(out: &mut Vec<u16>, work: &mut usize, units: &[u16]) -> Result<(), Fault> {
    charge(work, units.len())?;
    out.extend_from_slice(units);
    Ok(())
}

fn set_last_index(heap: &Heap, task: TaskId, re: &Value, index: usize) -> Result<(), Fault> {
    let setter = property::prepare_put(
        heap,
        task,
        re,
        &object::key("lastIndex"),
        Value::Number(index as f64),
        true,
    )?;
    debug_assert!(setter.is_none());
    Ok(())
}

fn substitute(
    text: &JsString,
    template: &JsString,
    m: &matcher::Captures,
    names: Option<&[Option<JsString>]>,
    out: &mut Vec<u16>,
    work: &mut usize,
) -> Result<(), Fault> {
    let source = text.code_units();
    let units = template.code_units();
    let whole = m.groups[0].as_ref().unwrap();
    let named = names.is_some_and(|names| names.iter().any(Option::is_some));
    let mut i = 0;
    while i < units.len() {
        charge(work, 1)?;
        if units[i] != 36 || i + 1 == units.len() {
            append(out, work, &units[i..i + 1])?;
            i += 1;
            continue;
        }
        let token = units[i + 1];
        let mut consumed = 2;
        match token {
            36 => append(out, work, &[36])?,
            38 => append(out, work, &source[whole.clone()])?,
            96 => append(out, work, &source[..whole.start])?,
            39 => append(out, work, &source[whole.end..])?,
            48..=57 => {
                let mut n = (token - 48) as usize;
                if let Some(&digit @ 48..=57) = units.get(i + 2) {
                    let double = n * 10 + (digit - 48) as usize;
                    if double > 0 && double < m.groups.len() {
                        n = double;
                        consumed = 3;
                    }
                }
                if n > 0 && n < m.groups.len() {
                    if let Some(span) = &m.groups[n] {
                        append(out, work, &source[span.clone()])?;
                    }
                } else {
                    append(out, work, &[36])?;
                    consumed = 1;
                }
            }
            60 if named => {
                let mut end = i + 2;
                while end < units.len() && units[end] != 62 {
                    charge(work, 1)?;
                    end += 1;
                }
                if end == units.len() {
                    append(out, work, &[36])?;
                    consumed = 1;
                } else {
                    let name = &units[i + 2..end];
                    for (index, capture_name) in names.unwrap().iter().enumerate() {
                        charge(work, 1)?;
                        if let Some(capture_name) = capture_name {
                            charge(work, capture_name.len() + name.len())?;
                            if capture_name.code_units() == name
                                && let Some(span) = &m.groups[index + 1]
                            {
                                append(out, work, &source[span.clone()])?;
                                break;
                            }
                        }
                    }
                    consumed = end + 1 - i;
                }
            }
            _ => {
                append(out, work, &[36])?;
                consumed = 1;
            }
        }
        i += consumed;
    }
    Ok(())
}
