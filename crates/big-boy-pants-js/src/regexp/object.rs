//! RegExp internal slots. Programs are immutable and contain no GC edges;
//! ordinary property storage (including lastIndex) remains task-owned.
use std::sync::Arc;

use super::{matcher, parse};
use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::value::{JsString, Value};
use crate::vm::{dispatch::Fault, property};

#[derive(Debug)]
pub(crate) struct Compiled {
    pub(crate) source: JsString,
    pub(crate) flags: parse::Flags,
    pub(crate) names: Vec<Option<JsString>>,
    pub(crate) program: matcher::Program,
}

#[derive(Clone, Debug)]
pub(crate) enum CompileError {
    Syntax(String),
    Limit,
}
impl CompileError {
    pub(crate) fn fault(&self) -> Fault {
        match self {
            Self::Syntax(s) => Fault::SyntaxError(s.clone()),
            Self::Limit => Fault::RangeError("regular expression step limit exceeded".into()),
        }
    }
}

pub(crate) fn compile(source: &JsString, flags: &JsString) -> Result<Arc<Compiled>, CompileError> {
    let pattern = parse::parse(source, flags).map_err(|e| {
        CompileError::Syntax(format!("{} at UTF-16 offset {}", e.message, e.offset))
    })?;
    let program = matcher::compile(&pattern).map_err(|e| match e {
        matcher::MatchError::LimitExceeded => CompileError::Limit,
        _ => CompileError::Syntax(e.to_string()),
    })?;
    Ok(Arc::new(Compiled {
        source: source.clone(),
        flags: pattern.flags,
        names: pattern.captures,
        program,
    }))
}

/// Bytecode constant encoding: flag code units, NUL separator, raw pattern.
/// Loading compiles this once, including a deferred catchable error result.
pub(crate) fn compile_literal(encoded: &JsString) -> Result<Arc<Compiled>, CompileError> {
    let units = encoded.code_units();
    let split = units
        .iter()
        .position(|&c| c == 0)
        .ok_or_else(|| CompileError::Syntax("invalid RegExp literal constant".into()))?;
    compile(
        &JsString::from_code_units(&units[split + 1..]).expect("substring fits"),
        &JsString::from_code_units(&units[..split]).expect("substring fits"),
    )
}

pub(crate) fn allocate(heap: &Heap, task: TaskId, compiled: Arc<Compiled>) -> Result<Value, Fault> {
    let proto = crate::realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("RegExp realm"))?
        .regexp_prototype;
    let mut o = OrdinaryObject::with_prototype(heap, task, proto).with_regexp(compiled);
    let key = heap
        .keys()
        .intern(&JsString::from("lastIndex"))
        .ok_or(Fault::OutOfMemory("RegExp key"))?;
    o.define_with(
        key,
        Value::Number(0.0),
        PropertyFlags {
            writable: true,
            enumerable: false,
            configurable: false,
        },
    );
    heap.alloc(HeapObject::Plain(o))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("RegExp object"))
}

pub(crate) fn data(
    heap: &Heap,
    task: TaskId,
    this: &Value,
) -> Result<Option<Arc<Compiled>>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    heap.read(*id, |o| match o {
        HeapObject::Plain(o) => {
            if o.regexp().is_some() && o.owner() != Some(task) {
                Err(Fault::ForeignObject {
                    owner: o.owner(),
                    caller: task,
                })
            } else {
                Ok(o.regexp().cloned())
            }
        }
        _ => Ok(None),
    })
    .unwrap_or(Err(Fault::Internal("RegExp reclaimed")))
}

pub(crate) fn receiver(heap: &Heap, task: TaskId, this: &Value) -> Result<Arc<Compiled>, Fault> {
    data(heap, task, this)?
        .ok_or_else(|| Fault::TypeError("RegExp method requires a RegExp receiver".into()))
}

pub(crate) const FLAG_NAMES: [(&str, char); 8] = [
    ("hasIndices", 'd'),
    ("global", 'g'),
    ("ignoreCase", 'i'),
    ("multiline", 'm'),
    ("dotAll", 's'),
    ("unicode", 'u'),
    ("unicodeSets", 'v'),
    ("sticky", 'y'),
];

pub(crate) fn flag(f: parse::Flags, c: char) -> bool {
    match c {
        'd' => f.has_indices,
        'g' => f.global,
        'i' => f.ignore_case,
        'm' => f.multiline,
        's' => f.dot_all,
        'u' => f.unicode,
        'v' => f.unicode_sets,
        'y' => f.sticky,
        _ => false,
    }
}

pub(crate) fn flags(f: parse::Flags) -> JsString {
    JsString::from(
        FLAG_NAMES
            .iter()
            .filter_map(|&(_, c)| flag(f, c).then_some(c))
            .collect::<String>()
            .as_str(),
    )
}

/// EscapeRegExpPattern preserves existing escapes, slash character classes,
/// raw NUL and lone surrogates. Only slash and line terminators need quoting.
pub(crate) fn source(source: &JsString) -> Result<JsString, Fault> {
    if source.is_empty() {
        return Ok(JsString::from("(?:)"));
    }
    let mut out = Vec::new();
    let mut escaped = false;
    let mut class = false;
    for &c in source.code_units().iter() {
        let extra = match c {
            10 | 13 => 2 - usize::from(escaped),
            0x2028 | 0x2029 => 6 - usize::from(escaped),
            47 if !escaped && !class => 2,
            _ => 1,
        };
        if out.len() > crate::value::string::MAX_STRING_LEN - extra {
            return Err(Fault::StringTooLong);
        }
        if matches!(c, 10 | 13 | 0x2028 | 0x2029) {
            // A literal backslash before a line terminator must not consume
            // the escape we insert (constructor patterns can contain both).
            if escaped {
                out.pop();
            }
            out.extend(
                match c {
                    10 => "\\n",
                    13 => "\\r",
                    0x2028 => "\\u2028",
                    _ => "\\u2029",
                }
                .encode_utf16(),
            );
            escaped = false;
            continue;
        }
        if !escaped && c == 47 && !class {
            out.push(92);
        }
        if !escaped && c == 91 {
            class = true;
        }
        if !escaped && c == 93 {
            class = false;
        }
        out.push(c);
        escaped = !escaped && c == 92;
    }
    JsString::from_code_units(out).ok_or(Fault::StringTooLong)
}

pub(crate) fn exec(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    input: &JsString,
    last: u64,
) -> Result<Value, Fault> {
    exec_with_capture_budget(heap, task, this, input, last, None)
}

/// Replacement shares its materialization allowance across all matches.
/// Charge before allocating any result or invoking a replacer.
pub(crate) fn exec_with_capture_budget(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    input: &JsString,
    last: u64,
    mut replacement_work: Option<&mut usize>,
) -> Result<Value, Fault> {
    let c = receiver(heap, task, this)?;
    let stateful = c.flags.global || c.flags.sticky;
    let start = if stateful {
        usize::try_from(last).unwrap_or(usize::MAX)
    } else {
        0
    };
    let (found, mut budget) = matcher::exec_with_budget(&c.program, input.code_units(), start)
        .map_err(|e| match e {
            matcher::MatchError::LimitExceeded => {
                Fault::RangeError("regular expression step limit exceeded".into())
            }
            _ => Fault::SyntaxError(e.to_string()),
        })?;
    if stateful {
        let end = found
            .as_ref()
            .and_then(|m| m.groups[0].as_ref())
            .map_or(0, |r| r.end);
        // lastIndex is a non-configurable own data property, never a setter.
        let setter = property::prepare_put(
            heap,
            task,
            this,
            &key("lastIndex"),
            Value::Number(end as f64),
            true,
        )?;
        debug_assert!(setter.is_none());
    }
    let Some(m) = found else {
        return Ok(Value::Null);
    };
    if let Some(work) = &mut replacement_work {
        let units = m.groups.iter().flatten().map(|r| r.len()).sum::<usize>() + m.groups.len() + 1;
        **work = work
            .checked_sub(units)
            .ok_or_else(|| Fault::RangeError("string replacement work limit exceeded".into()))?;
    }
    // Reserve every copied UTF-16 unit, including group zero and overlapping
    // lookahead/nested captures, against the allowance left by this search.
    // Do this before allocating any result, so a rejected match copies nothing.
    for range in m.groups.iter().flatten() {
        budget
            .charge(range.len())
            .map_err(|_| CompileError::Limit.fault())?;
    }
    let a = property::new_array(heap, task)?;
    // The matcher reserves the extra per-capture work for d within its search
    // allowance before we allocate any indices. This leaf has no safepoints;
    // the normal array/object edges trace all pairs once it returns.
    let indices = c
        .flags
        .has_indices
        .then(|| property::new_array(heap, task))
        .transpose()?;
    let mut groups = OrdinaryObject::new(heap, Some(task));
    let mut indices_groups = OrdinaryObject::new(heap, Some(task));
    let mut named = false;
    for (i, capture) in m.groups.iter().enumerate() {
        let v = capture.as_ref().map_or(Value::Undefined, |r| {
            Value::String(
                JsString::from_code_units(&input.code_units()[r.clone()]).expect("substring fits"),
            )
        });
        property::append(heap, task, &a, Some(v.clone()))?;
        let pair = if let Some(indices) = &indices {
            let pair = if let Some(range) = capture {
                let pair = property::new_array(heap, task)?;
                property::append(heap, task, &pair, Some(Value::Number(range.start as f64)))?;
                property::append(heap, task, &pair, Some(Value::Number(range.end as f64)))?;
                pair
            } else {
                Value::Undefined
            };
            property::append(heap, task, indices, Some(pair.clone()))?;
            pair
        } else {
            Value::Undefined
        };
        if i > 0
            && let Some(name) = &c.names[i - 1]
        {
            named = true;
            let k = heap
                .keys()
                .intern(name)
                .ok_or(Fault::OutOfMemory("capture name"))?;
            // Duplicate names in disjoint alternatives select the matched one.
            if !matches!(v, Value::Undefined) || groups.get_own(k).is_none() {
                groups.define_with(k, v, PropertyFlags::DATA);
                if indices.is_some() {
                    indices_groups.define_with(k, pair, PropertyFlags::DATA);
                }
            }
        }
    }
    let groups = if named {
        Value::Object(
            heap.alloc(HeapObject::Plain(groups))
                .ok_or(Fault::OutOfMemory("capture groups"))?,
        )
    } else {
        Value::Undefined
    };
    for (name, v) in [
        ("index", Value::Number(m.index as f64)),
        ("input", Value::String(input.clone())),
        ("groups", groups),
    ] {
        result_property(heap, task, &a, name, v)?;
    }
    if let Some(indices) = indices {
        let groups = if named {
            Value::Object(
                heap.alloc(HeapObject::Plain(indices_groups))
                    .ok_or(Fault::OutOfMemory("capture indices groups"))?,
            )
        } else {
            Value::Undefined
        };
        result_property(heap, task, &indices, "groups", groups)?;
        result_property(heap, task, &a, "indices", indices)?;
    }
    Ok(a)
}

fn result_property(
    heap: &Heap,
    task: TaskId,
    result: &Value,
    name: &str,
    value: Value,
) -> Result<(), Fault> {
    let Value::Object(id) = result else {
        unreachable!()
    };
    let desc = crate::object::PropertyDescriptor {
        value: Some(value),
        writable: Some(true),
        enumerable: Some(true),
        configurable: Some(true),
        ..Default::default()
    };
    property::define_own_property(heap, task, *id, &key(name), &desc)
}

pub(crate) fn key(name: &str) -> property::Key {
    property::Key::Name(JsString::from(name))
}
