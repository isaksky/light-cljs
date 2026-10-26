//! Resumable RegExp coercions and generic property access. Matching itself is
//! a bounded leaf (MAX_MATCH_STEPS), without JavaScript calls or GC edges.
use super::{Action, Input};
use crate::heap::Heap;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::regexp::object::{self, key};
use crate::value::{JsString, Value};
use crate::vm::{
    builtin::Builtin,
    dispatch::{self, Fault},
    property,
};

#[derive(Debug)]
pub(super) enum State {
    Constructor {
        pattern: Value,
        flags: Value,
        original: Value,
        identity: bool,
        branded: bool,
        is_regexp: bool,
        new_target: Option<Value>,
        prototype: Option<crate::heap::ObjectId>,
        stage: u8,
    },
    Exec {
        this: Value,
        input: Value,
        text: JsString,
        stage: u8,
    },
    Test {
        this: Value,
        input: Value,
        stage: u8,
    },
    ToString {
        this: Value,
        source: JsString,
        stage: u8,
    },
    Flags {
        this: Value,
        out: String,
        next: usize,
    },
}
impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match self {
            Self::Constructor {
                pattern,
                flags,
                original,
                new_target,
                prototype,
                ..
            } => {
                pattern.trace(visit);
                flags.trace(visit);
                original.trace(visit);
                if let Some(target) = new_target {
                    target.trace(visit);
                }
                if let Some(id) = prototype {
                    visit((*id).into());
                }
            }
            Self::Exec { this, input, .. } | Self::Test { this, input, .. } => {
                this.trace(visit);
                input.trace(visit);
            }
            Self::ToString { this, .. } | Self::Flags { this, .. } => this.trace(visit),
        }
    }
}

pub(super) fn start(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
) -> Result<Option<State>, Fault> {
    let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
    Ok(Some(match which {
        Builtin::RegExp => {
            let original = arg(0);
            let mut pattern = original.clone();
            let mut flags = arg(1);
            let identity = !construct && matches!(flags, Value::Undefined);
            let mut branded = false;
            if let Some(c) = object::data(heap, task, &original)? {
                branded = true;
                pattern = Value::String(c.source.clone());
                if matches!(flags, Value::Undefined) {
                    flags = Value::String(object::flags(c.flags));
                }
            }
            State::Constructor {
                pattern,
                flags,
                original,
                identity,
                branded,
                is_regexp: false,
                new_target: None,
                prototype: None,
                stage: 10,
            }
        }
        Builtin::RegExpExec if !construct => {
            object::receiver(heap, task, this)?;
            State::Exec {
                this: this.clone(),
                input: arg(0),
                text: JsString::from(""),
                stage: 0,
            }
        }
        Builtin::RegExpTest if !construct => {
            require_object(this)?;
            State::Test {
                this: this.clone(),
                input: arg(0),
                stage: 0,
            }
        }
        Builtin::RegExpToString if !construct => {
            require_object(this)?;
            State::ToString {
                this: this.clone(),
                source: JsString::from(""),
                stage: 0,
            }
        }
        Builtin::RegExpFlags if !construct => {
            require_object(this)?;
            State::Flags {
                this: this.clone(),
                out: String::new(),
                next: 0,
            }
        }
        _ => return Ok(None),
    }))
}

pub(crate) fn getter(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
) -> Result<Value, Fault> {
    let c = object::data(heap, task, this)?;
    let intrinsic = crate::realm::intrinsics(heap).ok_or(Fault::OutOfMemory("RegExp realm"))?;
    let prototype = matches!(this, Value::Object(id) if *id == intrinsic.regexp_prototype);
    match (which, c) {
        (Builtin::RegExpSource, Some(c)) => Ok(Value::String(object::source(heap, &c.source)?)),
        (Builtin::RegExpSource, None) if prototype => Ok(Value::String(JsString::from("(?:)"))),
        (Builtin::RegExpFlag(f), Some(c)) => Ok(Value::Bool(object::flag(c.flags, f))),
        (Builtin::RegExpFlag(_), None) if prototype => Ok(Value::Undefined),
        _ => Err(Fault::TypeError(
            "RegExp getter requires a RegExp receiver".into(),
        )),
    }
}

fn require_object(this: &Value) -> Result<(), Fault> {
    if matches!(this, Value::Object(_)) {
        Ok(())
    } else {
        Err(Fault::TypeError("RegExp method requires an object".into()))
    }
}
fn get(this: &Value, name: &str) -> Action {
    Action::Instruction {
        instr: crate::vm::instr::Instr::GetElem,
        operands: vec![this.clone(), Value::String(name.into())],
    }
}
fn read(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    name: &str,
) -> Result<Result<Value, Action>, Fault> {
    match property::read(heap, task, this, &key(name))?
        .unwrap_or(property::Read::Value(Value::Undefined))
    {
        property::Read::Value(v) => Ok(Ok(v)),
        property::Read::Getter(callee) => Ok(Err(Action::Call {
            callee,
            this: this.clone(),
            args: vec![],
        })),
    }
}
fn string(v: Value) -> Result<JsString, Fault> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err(Fault::Internal("RegExp string conversion missing")),
    }
}
impl State {
    pub(super) fn set_new_target(&mut self, target: Value) {
        let Self::Constructor { new_target, .. } = self else {
            unreachable!("newTarget belongs to the RegExp constructor");
        };
        *new_target = Some(target);
    }

    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        let mut v = match input {
            Input::Start => Value::Undefined,
            Input::CallResult(v) => v,
            _ => return Err(Fault::Internal("RegExp unexpected dependency")),
        };
        match self {
            Self::Constructor {
                pattern,
                flags,
                original,
                identity,
                branded,
                is_regexp,
                new_target,
                prototype,
                stage,
            } => loop {
                match *stage {
                    10 => {
                        *stage = 11;
                        if matches!(original, Value::Object(_)) {
                            let key =
                                property::Key::Symbol(crate::value::symbol::well_known("match"));
                            match property::read(heap, task, original, &key)? {
                                Some(property::Read::Getter(callee)) => {
                                    return Ok(Action::Call {
                                        callee,
                                        this: original.clone(),
                                        args: vec![],
                                    });
                                }
                                Some(property::Read::Value(value)) => v = value,
                                None => v = Value::Undefined,
                            }
                        } else {
                            v = Value::Undefined;
                        }
                    }
                    11 => {
                        *is_regexp = if matches!(v, Value::Undefined) {
                            *branded
                        } else {
                            v.truthy()
                        };
                        *identity &= *is_regexp;
                        *stage = 0;
                    }
                    0 => {
                        *stage = 1;
                        if *identity {
                            match read(heap, task, original, "constructor")? {
                                Ok(value) => v = value,
                                Err(a) => return Ok(a),
                            }
                        }
                    }
                    1 => {
                        if *identity {
                            let constructor = crate::realm::intrinsics(heap)
                                .ok_or(Fault::OutOfMemory("RegExp realm"))?
                                .global(crate::realm::Global::RegExp);
                            if crate::atom::same_value(&v, &constructor) {
                                return Ok(Action::Return(original.clone()));
                            }
                        }
                        if *is_regexp && !*branded {
                            *stage = 12;
                            match read(heap, task, original, "source")? {
                                Ok(value) => v = value,
                                Err(action) => return Ok(action),
                            }
                            continue;
                        }
                        *stage = 15;
                    }
                    2 => {
                        *pattern = v;
                        *stage = 3;
                        if matches!(flags, Value::Undefined) {
                            v = Value::String(JsString::from(""));
                        } else {
                            return Ok(Action::ToString(flags.clone()));
                        }
                    }
                    3 => {
                        let source = string(pattern.clone())?;
                        let flags = string(v)?;
                        let c = object::compile_accounted(heap, &source, &flags)
                            .map_err(|e| e.fault())?;
                        let result = object::allocate(heap, task, c)?;
                        if let Some(prototype) = prototype {
                            super::construct::initialize_prototype(
                                heap, task, &result, *prototype,
                            )?;
                        }
                        return Ok(Action::Return(result));
                    }
                    12 => {
                        *pattern = v;
                        *stage = 13;
                        if matches!(flags, Value::Undefined) {
                            match read(heap, task, original, "flags")? {
                                Ok(value) => v = value,
                                Err(action) => return Ok(action),
                            }
                        } else {
                            v = flags.clone();
                        }
                    }
                    13 => {
                        *flags = std::mem::replace(&mut v, Value::Undefined);
                        *stage = 15;
                    }
                    15 => {
                        // RegExpAlloc follows IsRegExp/source/flags reads and
                        // precedes RegExpInitialize's string conversions.
                        *stage = 16;
                        if let Some(target) = new_target {
                            return Ok(get(target, "prototype"));
                        }
                    }
                    16 => {
                        if new_target.is_some() {
                            *prototype = Some(match v {
                                Value::Object(id) => id,
                                _ => {
                                    crate::realm::intrinsics(heap)
                                        .ok_or(Fault::OutOfMemory("RegExp realm"))?
                                        .regexp_prototype
                                }
                            });
                        }
                        *stage = 14;
                        if matches!(pattern, Value::Undefined) {
                            v = Value::String("".into());
                        } else {
                            return Ok(Action::ToString(pattern.clone()));
                        }
                    }
                    14 => {
                        *stage = 2;
                    }
                    _ => return Err(Fault::Internal("RegExp constructor stage")),
                }
            },
            Self::Exec {
                this,
                input,
                text,
                stage,
            } => loop {
                match *stage {
                    0 => {
                        *stage = 1;
                        return Ok(Action::ToString(input.clone()));
                    }
                    1 => {
                        *text = string(v)?;
                        *stage = 2;
                        match read(heap, task, this, "lastIndex")? {
                            Ok(value) => v = value,
                            Err(a) => return Ok(a),
                        }
                    }
                    2 => {
                        *stage = 3;
                        return Ok(Action::ToNumberLimited(v, 65_536));
                    }
                    3 => {
                        return object::exec(heap, task, this, text, super::to_length(&v)?)
                            .map(Action::Return);
                    }
                    _ => return Err(Fault::Internal("RegExp exec stage")),
                }
            },
            Self::Test { this, input, stage } => {
                loop {
                    match *stage {
                        0 => {
                            *stage = 1;
                            return Ok(Action::ToString(input.clone()));
                        }
                        1 => {
                            *input = v;
                            *stage = 2;
                            match read(heap, task, this, "exec")? {
                                Ok(value) => v = value,
                                Err(a) => return Ok(a),
                            }
                        }
                        2 => {
                            *stage = 3;
                            let callee = if dispatch::callee(heap, &v).is_ok() {
                                v
                            } else {
                                object::receiver(heap, task, this)?;
                                // Fall back to the intrinsic exec when an own exec isn't callable.
                                let proto = Value::Object(
                                    crate::realm::intrinsics(heap)
                                        .ok_or(Fault::OutOfMemory("RegExp realm"))?
                                        .regexp_prototype,
                                );
                                property::get(heap, task, &proto, &key("exec"))?
                            };
                            return Ok(Action::Call {
                                callee,
                                this: this.clone(),
                                args: vec![input.clone()],
                            });
                        }
                        3 => {
                            return match v {
                                Value::Null => Ok(Action::Return(Value::Bool(false))),
                                Value::Object(_) => Ok(Action::Return(Value::Bool(true))),
                                _ => Err(Fault::TypeError(
                                    "RegExp exec must return an object or null".into(),
                                )),
                            };
                        }
                        _ => return Err(Fault::Internal("RegExp test stage")),
                    }
                }
            }
            Self::ToString {
                this,
                source,
                stage,
            } => loop {
                match *stage {
                    0 | 3 => {
                        let name = if *stage == 0 { "source" } else { "flags" };
                        *stage += 1;
                        match read(heap, task, this, name)? {
                            Ok(value) => v = value,
                            Err(a) => return Ok(a),
                        }
                    }
                    1 | 4 => {
                        *stage += 1;
                        return Ok(Action::ToString(v));
                    }
                    2 => {
                        *source = string(v)?;
                        v = Value::Undefined;
                        *stage = 3;
                    }
                    5 => {
                        let flags = string(v)?;
                        let s = JsString::from("/")
                            .concat_accounted(heap, source)?
                            .concat_accounted(heap, &JsString::from("/"))?
                            .concat_accounted(heap, &flags)?;
                        return Ok(Action::Return(Value::String(s)));
                    }
                    _ => return Err(Fault::Internal("RegExp toString stage")),
                }
            },
            Self::Flags { this, out, next } => loop {
                if *next > 0 && v.truthy() {
                    out.push(object::FLAG_NAMES[*next - 1].1);
                }
                if *next == object::FLAG_NAMES.len() {
                    return Ok(Action::Return(Value::String(JsString::from(out.as_str()))));
                }
                let name = object::FLAG_NAMES[*next].0;
                *next += 1;
                match read(heap, task, this, name)? {
                    Ok(value) => v = value,
                    Err(a) => return Ok(a),
                }
            },
        }
    }
}
