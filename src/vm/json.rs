//! Budgeted JSON parsing and serialization (clj-quickjs-159/-160). All
//! container state is explicit and traced; callbacks use native continuations.
//! QuickJS json_parse_value/json_next_token at 7d1c13c5d8f8 are semantic
//! references only. No upstream implementation is copied.

use super::{builtin, dispatch::Fault, property};
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId};
use crate::ids::TaskId;
use crate::value::string::NumberParse;
use crate::value::{JsString, Value};

// A catchable, documented resource limit, independent of the native stack.
const MAX_DEPTH: usize = 512;
// The shared leaf key interner hashes/compares the complete decoded name.
// Cap that work independently of the resumable string-value decoder.
const MAX_MEMBER_NAME_UNITS: usize = 4096;
const STEP: usize = 256;

#[derive(Clone, Copy, Debug)]
enum Expect {
    ArrayFirst,
    ArrayValue,
    ArrayComma,
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}

#[derive(Debug)]
struct Container {
    value: Value,
    expect: Expect,
    key: Option<JsString>,
}

#[derive(Debug)]
enum Token {
    Syntax,
    StringScan {
        start: usize,
        key: bool,
        decoded: usize,
    },
    StringDecode {
        next: usize,
        end: usize,
        key: bool,
        units: Vec<u16>,
    },
    NumberScan {
        start: usize,
        state: u8,
    },
    NumberValue(Box<NumberParse>),
}

#[derive(Debug)]
pub(super) struct Parser {
    text: JsString,
    next: usize,
    line: usize,
    column: usize,
    cr: bool,
    stack: Vec<Container>,
    result: Option<Value>,
    token: Token,
}

impl Trace for Parser {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for container in &self.stack {
            container.value.trace(visit);
        }
        if let Some(value) = &self.result {
            value.trace(visit);
        }
    }
}

impl Parser {
    pub(super) fn new(text: JsString) -> Self {
        Self {
            text,
            next: 0,
            line: 1,
            column: 1,
            cr: false,
            stack: Vec::new(),
            result: None,
            token: Token::Syntax,
        }
    }

    fn peek(&self) -> Option<u16> {
        self.text.code_units().get(self.next).copied()
    }

    fn consume(&mut self) {
        let c = self.peek().expect("consuming JSON input");
        self.next += 1;
        match c {
            13 => {
                self.line += 1;
                self.column = 1;
            }
            10 if self.cr => {}
            10 => {
                self.line += 1;
                self.column = 1;
            }
            _ => self.column += 1,
        }
        self.cr = c == 13;
    }

    fn error(&self, problem: &str) -> Fault {
        Fault::SyntaxError(format!(
            "JSON.parse: {problem} at line {} column {} (UTF-16 offset {})",
            self.line, self.column, self.next
        ))
    }

    fn accept(&mut self, heap: &Heap, task: TaskId, value: Value) -> Result<(), Fault> {
        if let Some(top) = self.stack.last_mut() {
            match top.expect {
                Expect::ArrayFirst | Expect::ArrayValue => {
                    property::append(heap, task, &top.value, Some(value))?;
                    top.expect = Expect::ArrayComma;
                }
                Expect::ObjectValue => {
                    property::define_field(
                        heap,
                        task,
                        &top.value,
                        &top.key.take().expect("JSON object key"),
                        value,
                    )?;
                    top.expect = Expect::ObjectComma;
                }
                _ => return Err(Fault::Internal("unexpected JSON value destination")),
            }
        } else {
            self.result = Some(value);
        }
        Ok(())
    }

    fn close(&mut self, heap: &Heap, task: TaskId) -> Result<(), Fault> {
        self.consume();
        let container = self.stack.pop().expect("JSON container");
        self.accept(heap, task, container.value)
    }

    fn value(&mut self, heap: &Heap, task: TaskId) -> Result<(), Fault> {
        match self.peek() {
            Some(91 | 123) => {
                if self.stack.len() == MAX_DEPTH {
                    return Err(self.error("nesting limit of 512 exceeded"));
                }
                let array = self.peek() == Some(91);
                let value = if array {
                    property::new_array(heap, task)?
                } else {
                    builtin::new_object(heap, task)?
                };
                self.consume();
                self.stack.push(Container {
                    value,
                    expect: if array {
                        Expect::ArrayFirst
                    } else {
                        Expect::ObjectFirst
                    },
                    key: None,
                });
            }
            Some(34) => {
                self.consume();
                self.token = Token::StringScan {
                    start: self.next,
                    key: false,
                    decoded: 0,
                };
            }
            Some(45 | 48..=57) => {
                self.token = Token::NumberScan {
                    start: self.next,
                    state: 0,
                };
            }
            Some(116 | 102 | 110) => {
                let (word, value) = match self.peek() {
                    Some(116) => ("true", Value::Bool(true)),
                    Some(102) => ("false", Value::Bool(false)),
                    _ => ("null", Value::Null),
                };
                for c in word.bytes() {
                    if self.peek() != Some(u16::from(c)) {
                        return Err(self.error("invalid literal"));
                    }
                    self.consume();
                }
                self.accept(heap, task, value)?;
            }
            _ => return Err(self.error("expected a JSON value")),
        }
        Ok(())
    }

    /// At most 256 scanning/decoding/grammar operations plus 256 numeric
    /// accumulator units per step. Escape
    /// lookahead is at most six units. Numeric conversion uses the existing
    /// bounded, correctly rounded decimal accumulator (1100 retained digits).
    /// String bodies are sized once after scanning and transferred without
    /// a final linear copy. Property insertion uses the ordinary leaf API.
    pub(super) fn advance(&mut self, heap: &Heap, task: TaskId) -> Result<Option<Value>, Fault> {
        for _ in 0..STEP {
            match std::mem::replace(&mut self.token, Token::Syntax) {
                Token::Syntax => {
                    if matches!(self.peek(), Some(9 | 10 | 13 | 32)) {
                        self.consume();
                        continue;
                    }
                    match self.stack.last().map(|top| top.expect) {
                        None if self.result.is_some() => {
                            if self.peek().is_some() {
                                return Err(self.error("unexpected trailing character"));
                            }
                            return Ok(self.result.take());
                        }
                        Some(Expect::ArrayFirst) if self.peek() == Some(93) => {
                            self.close(heap, task)?
                        }
                        Some(Expect::ArrayComma) => match self.peek() {
                            Some(93) => self.close(heap, task)?,
                            Some(44) => {
                                self.consume();
                                self.stack.last_mut().unwrap().expect = Expect::ArrayValue;
                            }
                            _ => return Err(self.error("expected ',' or ']'")),
                        },
                        Some(Expect::ObjectFirst) if self.peek() == Some(125) => {
                            self.close(heap, task)?
                        }
                        Some(Expect::ObjectFirst | Expect::ObjectKey) => {
                            if self.peek() != Some(34) {
                                return Err(self.error("expected a double-quoted property name"));
                            }
                            self.consume();
                            self.token = Token::StringScan {
                                start: self.next,
                                key: true,
                                decoded: 0,
                            };
                        }
                        Some(Expect::ObjectColon) => {
                            if self.peek() != Some(58) {
                                return Err(self.error("expected ':'"));
                            }
                            self.consume();
                            self.stack.last_mut().unwrap().expect = Expect::ObjectValue;
                        }
                        Some(Expect::ObjectComma) => match self.peek() {
                            Some(125) => self.close(heap, task)?,
                            Some(44) => {
                                self.consume();
                                self.stack.last_mut().unwrap().expect = Expect::ObjectKey;
                            }
                            _ => return Err(self.error("expected ',' or '}'")),
                        },
                        _ => self.value(heap, task)?,
                    }
                }
                Token::StringScan {
                    start,
                    key,
                    decoded,
                } => {
                    if key && decoded == MAX_MEMBER_NAME_UNITS && self.peek() != Some(34) {
                        return Err(self.error("member name limit of 4096 UTF-16 units exceeded"));
                    }
                    match self.peek() {
                        Some(34) => {
                            let end = self.next;
                            self.consume();
                            let mut units = Vec::new();
                            units
                                .try_reserve_exact(end - start)
                                .map_err(|_| Fault::OutOfMemory("JSON string buffer"))?;
                            self.token = Token::StringDecode {
                                next: start,
                                end,
                                key,
                                units,
                            };
                            continue;
                        }
                        Some(92) => {
                            self.consume();
                            match self.peek() {
                                Some(34 | 92 | 47 | 98 | 102 | 110 | 114 | 116) => self.consume(),
                                Some(117) => {
                                    self.consume();
                                    for _ in 0..4 {
                                        if self.peek().and_then(hex).is_none() {
                                            return Err(self
                                                .error("expected four hexadecimal escape digits"));
                                        }
                                        self.consume();
                                    }
                                }
                                _ => return Err(self.error("invalid string escape")),
                            }
                        }
                        Some(0..=31) => {
                            return Err(self.error("unescaped control character in string"));
                        }
                        None => return Err(self.error("unterminated string")),
                        Some(_) => self.consume(),
                    }
                    self.token = Token::StringScan {
                        start,
                        key,
                        decoded: decoded + 1,
                    };
                }
                Token::StringDecode {
                    mut next,
                    end,
                    key,
                    mut units,
                } => {
                    if next == end {
                        let text = JsString::from_owned_code_units(units)
                            .ok_or_else(|| Fault::RangeError("JSON string too long".to_string()))?;
                        if key {
                            let top = self.stack.last_mut().expect("JSON object");
                            top.key = Some(text);
                            top.expect = Expect::ObjectColon;
                        } else {
                            self.accept(heap, task, Value::String(text))?;
                        }
                        continue;
                    }
                    let input = self.text.code_units();
                    let mut c = input[next];
                    next += 1;
                    if c == 92 {
                        c = match input[next] {
                            34 => 34,
                            92 => 92,
                            47 => 47,
                            98 => 8,
                            102 => 12,
                            110 => 10,
                            114 => 13,
                            116 => 9,
                            117 => {
                                let mut unit = 0;
                                for &digit in &input[next + 1..next + 5] {
                                    unit = unit * 16 + hex(digit).expect("validated JSON escape");
                                }
                                next += 4;
                                unit
                            }
                            _ => unreachable!("validated JSON escape"),
                        };
                        next += 1;
                    }
                    units.push(c);
                    self.token = Token::StringDecode {
                        next,
                        end,
                        key,
                        units,
                    };
                }
                Token::NumberScan { start, state } => {
                    let digit = matches!(self.peek(), Some(48..=57));
                    let next_state = match (state, self.peek()) {
                        (0, Some(45)) => Some(1),
                        (0 | 1, Some(48)) => Some(2),
                        (0 | 1, Some(49..=57)) | (3, _) if digit => Some(3),
                        (2 | 3, Some(46)) => Some(4),
                        (4 | 5, _) if digit => Some(5),
                        (2 | 3 | 5, Some(69 | 101)) => Some(6),
                        (6, Some(43 | 45)) => Some(7),
                        (6..=8, _) if digit => Some(8),
                        _ => None,
                    };
                    if let Some(state) = next_state {
                        self.consume();
                        self.token = Token::NumberScan { start, state };
                    } else {
                        if !matches!(state, 2 | 3 | 5 | 8) {
                            return Err(self.error("incomplete JSON number"));
                        }
                        self.token = Token::NumberValue(Box::new(NumberParse::range(
                            self.text.clone(),
                            None,
                            start,
                            self.next,
                        )));
                    }
                }
                Token::NumberValue(mut parse) => {
                    if let Some(number) = parse.advance(STEP) {
                        self.accept(heap, task, Value::Number(number))?;
                    } else {
                        self.token = Token::NumberValue(parse);
                    }
                    // Numeric scanning consumes its own full step budget.
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }
}

fn hex(c: u16) -> Option<u16> {
    match c {
        48..=57 => Some(c - 48),
        65..=70 => Some(c - 65 + 10),
        97..=102 => Some(c - 97 + 10),
        _ => None,
    }
}

// JSON.stringify (clj-quickjs-160). SerializeJSONProperty and QuoteJSONString
// are implemented afresh; QuickJS js_json_check/js_json_to_str are semantic
// references. The explicit stack, callbacks and output builder are task-owned.
use super::{dispatch, keys};
use crate::heap::HeapObject;
use crate::object::PrimitiveData;
use crate::value::string::{MAX_STRING_LEN, StringBuild, number_to_string};
use std::collections::HashSet;

// Own-key collection is a shared leaf API. Bound its scan/sort here; arrays
// stream their indices and have no corresponding element-count limit.
const MAX_OBJECT_PROPERTIES: usize = 4096;
// StringToNumber remains a leaf; bound strings returned by a Number
// wrapper's observable ToPrimitive before invoking its scanner/parser.
pub(super) const MAX_NUMBER_CONVERSION_UNITS: usize = 4096;

#[derive(Debug)]
enum Members {
    Array {
        next: u32,
        len: u32,
    },
    Object {
        keys: std::vec::IntoIter<keys::OwnKey>,
        emitted: bool,
    },
}

#[derive(Debug)]
struct StringifyContainer {
    value: Value,
    members: Members,
}

#[derive(Debug)]
enum StringifyStage {
    Prepare,
    Method,
    ToJsonResult,
    Unwrap,
    Converted,
    Ready,
    Emit,
    Next,
    PropertyResult,
    Quote {
        text: JsString,
        next: usize,
        key: bool,
    },
    Build(StringBuild),
}

pub(super) enum StringifyStep {
    Continue,
    Call {
        callee: Value,
        this: Value,
        args: Vec<Value>,
    },
    ToNumber(Value),
    ToString(Value),
    Return(Value),
}

#[derive(Debug)]
pub(super) struct Stringifier {
    value: Value,
    key: JsString,
    stack: Vec<StringifyContainer>,
    active: HashSet<ObjectId>,
    stage: StringifyStage,
    parts: Vec<(JsString, usize, usize)>,
    units: usize,
}

impl Trace for Stringifier {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.value.trace(visit);
        for container in &self.stack {
            container.value.trace(visit);
        }
    }
}

impl Stringifier {
    pub(super) fn new(value: Value) -> Self {
        Self {
            value,
            key: JsString::from(""),
            stack: Vec::new(),
            active: HashSet::new(),
            stage: StringifyStage::Prepare,
            parts: Vec::new(),
            units: 0,
        }
    }

    fn append(&mut self, text: JsString) -> Result<(), Fault> {
        self.units = self
            .units
            .checked_add(text.len())
            .filter(|n| *n <= MAX_STRING_LEN)
            .ok_or_else(|| Fault::RangeError("JSON.stringify string too long".into()))?;
        if !text.is_empty() {
            let len = text.len();
            self.parts.push((text, 0, len));
        }
        Ok(())
    }

    fn method(&mut self, heap: &Heap, method: Value) -> StringifyStep {
        if dispatch::callee(heap, &method).is_ok() {
            self.stage = StringifyStage::ToJsonResult;
            StringifyStep::Call {
                callee: method,
                this: self.value.clone(),
                args: vec![Value::String(self.key.clone())],
            }
        } else {
            self.stage = StringifyStage::Unwrap;
            StringifyStep::Continue
        }
    }

    fn container(&mut self, heap: &Heap, task: TaskId, id: ObjectId) -> Result<(), Fault> {
        if self.active.contains(&id) {
            return Err(Fault::TypeError("JSON.stringify circular reference".into()));
        }
        if self.stack.len() == MAX_DEPTH {
            return Err(Fault::RangeError(
                "JSON.stringify nesting limit of 512 exceeded".into(),
            ));
        }
        let typed_length = crate::object::typed_array::view(heap, task, id)?
            .map(|v| v.live_length(heap))
            .transpose()?
            .unwrap_or(0);
        let len = heap
            .read(id, |o| match o {
                HeapObject::Array(a) => {
                    if let Some(owner) = a.object().owner()
                        && owner != task
                    {
                        return Err(Fault::ForeignObject {
                            owner: Some(owner),
                            caller: task,
                        });
                    }
                    Ok(Some(a.length()))
                }
                HeapObject::Plain(o)
                    if o.properties().take(MAX_OBJECT_PROPERTIES + 1).count()
                        > MAX_OBJECT_PROPERTIES =>
                {
                    Err(Fault::RangeError(
                        "JSON.stringify object property limit of 4096 exceeded".into(),
                    ))
                }
                HeapObject::ArrayBuffer(b)
                    if b.object
                        .properties()
                        .take(MAX_OBJECT_PROPERTIES + 1)
                        .count()
                        > MAX_OBJECT_PROPERTIES =>
                {
                    Err(Fault::RangeError(
                        "JSON.stringify object property limit of 4096 exceeded".into(),
                    ))
                }
                HeapObject::TypedArray(a)
                    if typed_length
                        + a.object
                            .properties()
                            .take(MAX_OBJECT_PROPERTIES + 1)
                            .count()
                        > MAX_OBJECT_PROPERTIES =>
                {
                    Err(Fault::RangeError(
                        "JSON.stringify object property limit of 4096 exceeded".into(),
                    ))
                }
                _ => Ok(None),
            })
            .ok_or(Fault::Internal("JSON.stringify object reclaimed"))??;
        let members = if let Some(len) = len {
            self.append(JsString::from("["))?;
            Members::Array { next: 0, len }
        } else {
            let mut keys = keys::own_property_keys(heap, task, id, keys::KeyFilter::Enumerable)?;
            keys.retain(|key| !matches!(key.key, property::Key::Symbol(_)));
            self.append(JsString::from("{"))?;
            Members::Object {
                keys: keys.into_iter(),
                emitted: false,
            }
        };
        self.active.insert(id);
        self.stack.push(StringifyContainer {
            value: self.value.clone(),
            members,
        });
        self.value = Value::Undefined;
        self.stage = StringifyStage::Next;
        Ok(())
    }

    /// One transition, one property/callback, or at most 256 quoted/copied
    /// code units. No recursive traversal, output-sized copy or string scan.
    pub(super) fn advance(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Option<Value>,
    ) -> Result<StringifyStep, Fault> {
        use StringifyStage as S;
        use StringifyStep as A;
        match std::mem::replace(&mut self.stage, S::Next) {
            S::Prepare => {
                if matches!(self.value, Value::Object(_)) {
                    match property::read(
                        heap,
                        task,
                        &self.value,
                        &property::Key::Name(JsString::from("toJSON")),
                    )? {
                        Some(property::Read::Getter(callee)) => {
                            self.stage = S::Method;
                            return Ok(A::Call {
                                callee,
                                this: self.value.clone(),
                                args: vec![],
                            });
                        }
                        Some(property::Read::Value(method)) => return Ok(self.method(heap, method)),
                        None => {}
                    }
                }
                self.stage = S::Unwrap;
            }
            S::Method => {
                return Ok(self.method(
                    heap,
                    input.ok_or(Fault::Internal("JSON.stringify missing method"))?,
                ));
            }
            S::ToJsonResult => {
                self.value =
                    input.ok_or(Fault::Internal("JSON.stringify missing toJSON result"))?;
                self.stage = S::Unwrap;
            }
            S::Unwrap => {
                self.stage = S::Ready;
                match builtin::wrapper_data(heap, task, &self.value)? {
                    Some(PrimitiveData::Number(_)) => {
                        self.stage = S::Converted;
                        return Ok(A::ToNumber(self.value.clone()));
                    }
                    Some(PrimitiveData::String(_)) => {
                        self.stage = S::Converted;
                        return Ok(A::ToString(self.value.clone()));
                    }
                    None => {}
                }
            }
            S::Converted => {
                self.value =
                    input.ok_or(Fault::Internal("JSON.stringify missing wrapper conversion"))?;
                self.stage = S::Ready;
            }
            S::Ready => {
                let omitted = matches!(self.value, Value::Undefined | Value::Symbol(_))
                    || dispatch::callee(heap, &self.value).is_ok();
                if omitted {
                    match self.stack.last().map(|c| &c.members) {
                        None => return Ok(A::Return(Value::Undefined)),
                        Some(Members::Object { .. }) => return Ok(A::Continue),
                        Some(Members::Array { .. }) => self.value = Value::Null,
                    }
                }
                if let Some(StringifyContainer {
                    members: Members::Object { emitted, .. },
                    ..
                }) = self.stack.last_mut()
                {
                    let comma = *emitted;
                    *emitted = true;
                    if comma {
                        self.append(JsString::from(","))?;
                    }
                    self.append(JsString::from("\""))?;
                    self.stage = S::Quote {
                        text: self.key.clone(),
                        next: 0,
                        key: true,
                    };
                } else {
                    self.stage = S::Emit;
                }
            }
            S::Emit => match &self.value {
                Value::String(text) => {
                    let text = text.clone();
                    self.append(JsString::from("\""))?;
                    self.stage = S::Quote {
                        text,
                        next: 0,
                        key: false,
                    };
                }
                Value::Number(n) => self.append(JsString::from(
                    if n.is_finite() {
                        number_to_string(*n)
                    } else {
                        "null".into()
                    }
                    .as_str(),
                ))?,
                Value::Null => self.append(JsString::from("null"))?,
                Value::Bool(b) => self.append(JsString::from(if *b { "true" } else { "false" }))?,
                Value::Object(id) => self.container(heap, task, *id)?,
                Value::BigInt(_) => {
                    return Err(Fault::TypeError(
                        "JSON.stringify cannot serialize BigInt".into(),
                    ));
                }
                Value::Undefined | Value::Symbol(_) => {
                    return Err(Fault::Internal("JSON.stringify emitting omitted value"));
                }
            },
            S::Next => {
                self.value = Value::Undefined;
                let Some(top) = self.stack.last_mut() else {
                    self.stage = S::Build(
                        StringBuild::with_length(std::mem::take(&mut self.parts), self.units)
                            .ok_or(Fault::OutOfMemory("JSON.stringify output buffer"))?,
                    );
                    return Ok(A::Continue);
                };
                let (key, comma) = match &mut top.members {
                    Members::Array { next, len } if *next < *len => {
                        let key = property::Key::Index(*next);
                        let comma = *next != 0;
                        *next += 1;
                        (Some(key), comma)
                    }
                    Members::Array { .. } => (None, false),
                    Members::Object { keys, .. } => (keys.next().map(|k| k.key), false),
                };
                let Some(key) = key else {
                    let container = self.stack.pop().unwrap();
                    let Value::Object(id) = container.value else {
                        unreachable!()
                    };
                    self.active.remove(&id);
                    self.append(JsString::from(
                        if matches!(container.members, Members::Array { .. }) {
                            "]"
                        } else {
                            "}"
                        },
                    ))?;
                    return Ok(A::Continue);
                };
                let holder = top.value.clone();
                self.key = match &key {
                    property::Key::Index(n) => JsString::from(n.to_string().as_str()),
                    property::Key::Name(name) => name.clone(),
                    property::Key::Symbol(_) => {
                        return Err(Fault::Internal("JSON.stringify enumerated a symbol key"));
                    }
                };
                if self.key.len() > MAX_MEMBER_NAME_UNITS {
                    return Err(Fault::RangeError(
                        "JSON.stringify member name limit of 4096 UTF-16 units exceeded".into(),
                    ));
                }
                if comma {
                    self.append(JsString::from(","))?;
                }
                match property::read(heap, task, &holder, &key)? {
                    Some(property::Read::Getter(callee)) => {
                        self.stage = S::PropertyResult;
                        return Ok(A::Call {
                            callee,
                            this: holder,
                            args: vec![],
                        });
                    }
                    Some(property::Read::Value(value)) => self.value = value,
                    None => {}
                }
                self.stage = S::Prepare;
            }
            S::PropertyResult => {
                self.value =
                    input.ok_or(Fault::Internal("JSON.stringify missing getter result"))?;
                self.stage = S::Prepare;
            }
            S::Quote {
                text,
                mut next,
                key,
            } => {
                let mut units = Vec::with_capacity(STEP * 6);
                let source = text.code_units();
                for _ in 0..STEP {
                    let Some(&c) = source.get(next) else {
                        break;
                    };
                    next += 1;
                    let escape = match c {
                        34 => Some(34),
                        92 => Some(92),
                        8 => Some(98),
                        12 => Some(102),
                        10 => Some(110),
                        13 => Some(114),
                        9 => Some(116),
                        _ => None,
                    };
                    if let Some(escaped) = escape {
                        units.extend([92, escaped]);
                    } else if c <= 31
                        || (0xd800..=0xdfff).contains(&c)
                            && !((0xd800..=0xdbff).contains(&c)
                                && source
                                    .get(next)
                                    .is_some_and(|n| (0xdc00..=0xdfff).contains(n))
                                || (0xdc00..=0xdfff).contains(&c)
                                    && next >= 2
                                    && (0xd800..=0xdbff).contains(&source[next - 2]))
                    {
                        const HEX: &[u8] = b"0123456789abcdef";
                        units.extend([92, 117]);
                        for shift in [12, 8, 4, 0] {
                            units.push(u16::from(HEX[((c >> shift) & 15) as usize]));
                        }
                    } else {
                        units.push(c);
                    }
                }
                self.append(
                    JsString::from_owned_code_units(units).expect("bounded JSON quote chunk"),
                )?;
                if next == source.len() {
                    self.append(JsString::from(if key { "\":" } else { "\"" }))?;
                    self.stage = if key { S::Emit } else { S::Next };
                } else {
                    self.stage = S::Quote { text, next, key };
                }
            }
            S::Build(mut build) => {
                if let Some(text) = build.advance(STEP) {
                    return Ok(A::Return(Value::String(text)));
                }
                self.stage = S::Build(build);
            }
        }
        Ok(A::Continue)
    }
}
