//! Budgeted JSON parsing and serialization (clj-quickjs-159/-160). All
//! container state is explicit and traced; callbacks use native continuations.
//! QuickJS json_parse_value/json_next_token at 7d1c13c5d8f8 are semantic
//! references only. No upstream implementation is copied.

use super::{builtin, dispatch::Fault, property};
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId};
use crate::ids::TaskId;
use crate::value::string::{NumberParse, StringFlatten};
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
    Ready(Lexeme, Position),
    Identifier(Position),
    StringScan {
        start: usize,
        position: Position,
        key: bool,
        decoded: usize,
    },
    StringDecode {
        position: Position,
        next: usize,
        end: usize,
        units: Vec<u16>,
    },
    NumberScan {
        start: usize,
        position: Position,
        state: u8,
    },
    NumberValue(Box<NumberParse>, Position),
}

#[derive(Clone, Copy, Debug)]
struct Position {
    offset: usize,
    line: usize,
    column: usize,
}

#[derive(Debug)]
enum Lexeme {
    Value(Value),
    String(JsString),
    Punct(u16),
    Identifier,
    End,
}

#[derive(Debug)]
pub(crate) struct Parser {
    text: JsString,
    flatten: Option<StringFlatten>,
    next: usize,
    line: usize,
    column: usize,
    cr: bool,
    stack: Vec<Container>,
    result: Option<Value>,
    token: Token,
    string_reservation: Option<std::sync::Arc<crate::heap::budget::ByteReservation>>,
}

impl Trace for Parser {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        for container in &self.stack {
            container.value.trace(visit);
        }
        if let Some(value) = &self.result {
            value.trace(visit);
        }
        if let Token::Ready(Lexeme::Value(value), _) = &self.token {
            value.trace(visit);
        }
    }
}

impl Parser {
    pub(crate) fn new(heap: &Heap, text: JsString) -> Result<Self, Fault> {
        Ok(Self {
            text: JsString::from(""),
            flatten: Some(StringFlatten::new_accounted(heap, text)?),
            next: 0,
            line: 1,
            column: 1,
            cr: false,
            stack: Vec::new(),
            result: None,
            token: Token::Syntax,
            string_reservation: None,
        })
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

    fn position(&self) -> Position {
        Position {
            offset: self.next,
            line: self.line,
            column: self.column,
        }
    }

    fn error_at(&self, position: Position, message: &str) -> Fault {
        Fault::JsonSyntaxError {
            message: message.to_string(),
            line: position.line,
            column: position.column,
        }
    }

    fn error(&self, message: &str) -> Fault {
        self.error_at(self.position(), message)
    }

    fn unexpected(&self, position: Position, token: &Lexeme) -> Fault {
        if matches!(token, Lexeme::End) {
            return self.error_at(position, "Unexpected end of JSON input");
        }
        // QuickJS formats through a 256-byte error buffer. Identifier tokens
        // are ASCII; all other unexpected tokens are one ASCII unit. Bound
        // message construction even for an input-sized invalid identifier.
        let mut message = String::from("unexpected token: '");
        for &unit in self.text.code_units()[position.offset..self.next]
            .iter()
            .take(255 - message.len())
        {
            if unit == 0 {
                break;
            }
            message.push(char::from_u32(u32::from(unit)).expect("ASCII JSON token"));
        }
        if message.len() < 255 {
            message.push('\'');
        }
        self.error_at(position, &message)
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
        let container = self.stack.pop().expect("JSON container");
        self.accept(heap, task, container.value)
    }

    fn grammar(
        &mut self,
        heap: &Heap,
        task: TaskId,
        token: Lexeme,
        position: Position,
    ) -> Result<Option<Value>, Fault> {
        match self.stack.last().map(|top| top.expect) {
            None if self.result.is_some() => {
                return if matches!(token, Lexeme::End) {
                    Ok(self.result.take())
                } else {
                    Err(self.error_at(position, "unexpected data at the end"))
                };
            }
            Some(Expect::ArrayFirst) if matches!(token, Lexeme::Punct(93)) => {
                self.close(heap, task)?;
                return Ok(None);
            }
            Some(Expect::ArrayComma) => {
                match token {
                    Lexeme::Punct(93) => self.close(heap, task)?,
                    Lexeme::Punct(44) => self.stack.last_mut().unwrap().expect = Expect::ArrayValue,
                    _ => return Err(self.error_at(position, "expecting ']'")),
                }
                return Ok(None);
            }
            Some(Expect::ObjectFirst) if matches!(token, Lexeme::Punct(125)) => {
                self.close(heap, task)?;
                return Ok(None);
            }
            Some(Expect::ObjectFirst | Expect::ObjectKey) => {
                let Lexeme::String(key) = token else {
                    return Err(self.error_at(position, "expecting property name"));
                };
                let top = self.stack.last_mut().unwrap();
                top.key = Some(key);
                top.expect = Expect::ObjectColon;
                return Ok(None);
            }
            Some(Expect::ObjectColon) => {
                if !matches!(token, Lexeme::Punct(58)) {
                    return Err(self.error_at(position, "expecting ':'"));
                }
                self.stack.last_mut().unwrap().expect = Expect::ObjectValue;
                return Ok(None);
            }
            Some(Expect::ObjectComma) => {
                match token {
                    Lexeme::Punct(125) => self.close(heap, task)?,
                    Lexeme::Punct(44) => self.stack.last_mut().unwrap().expect = Expect::ObjectKey,
                    _ => return Err(self.error_at(position, "expecting '}'")),
                }
                return Ok(None);
            }
            _ => {}
        }
        match token {
            Lexeme::Punct(c @ (91 | 123)) => {
                if self.stack.len() == MAX_DEPTH {
                    return Err(self.error_at(position, "nesting limit of 512 exceeded"));
                }
                let array = c == 91;
                let value = if array {
                    property::new_array(heap, task)?
                } else {
                    builtin::new_object(heap, task)?
                };
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
            Lexeme::String(text) => self.accept(heap, task, Value::String(text))?,
            Lexeme::Value(value) => self.accept(heap, task, value)?,
            _ => return Err(self.unexpected(position, &token)),
        }
        Ok(None)
    }

    /// At most 256 scanning/decoding/grammar operations plus 256 numeric
    /// accumulator units per step. Escape
    /// lookahead is at most six units. Numeric conversion uses the existing
    /// bounded, correctly rounded decimal accumulator (1100 retained digits).
    /// String bodies are sized once after scanning and transferred without
    /// a final linear copy. Property insertion uses the ordinary leaf API.
    pub(crate) fn advance(&mut self, heap: &Heap, task: TaskId) -> Result<Option<Value>, Fault> {
        if let Some(flatten) = &mut self.flatten {
            if let Some(text) = flatten.advance(STEP) {
                self.text = text;
                self.flatten = None;
            }
            return Ok(None);
        }
        for _ in 0..STEP {
            match std::mem::replace(&mut self.token, Token::Syntax) {
                Token::Syntax => {
                    if matches!(self.peek(), Some(9 | 10 | 13 | 32)) {
                        self.consume();
                        continue;
                    }
                    let position = self.position();
                    self.token = match self.peek() {
                        Some(34) => {
                            let key = matches!(
                                self.stack.last().map(|top| top.expect),
                                Some(Expect::ObjectFirst | Expect::ObjectKey)
                            );
                            self.consume();
                            Token::StringScan {
                                start: self.next,
                                position,
                                key,
                                decoded: 0,
                            }
                        }
                        Some(45 | 48..=57) => Token::NumberScan {
                            start: self.next,
                            position,
                            state: 0,
                        },
                        Some(65..=90 | 97..=122 | 36 | 95) => Token::Identifier(position),
                        Some(128..=65535) => return Err(self.error("unexpected character")),
                        Some(c) => {
                            self.consume();
                            Token::Ready(Lexeme::Punct(c), position)
                        }
                        None => Token::Ready(Lexeme::End, position),
                    };
                }
                Token::Ready(token, position) => {
                    if let Some(value) = self.grammar(heap, task, token, position)? {
                        return Ok(Some(value));
                    }
                }
                Token::Identifier(position) => {
                    if matches!(self.peek(), Some(65..=90 | 97..=122 | 48..=57 | 36 | 95)) {
                        self.consume();
                        self.token = Token::Identifier(position);
                    } else {
                        let word = &self.text.code_units()[position.offset..self.next];
                        let token = match word {
                            [116, 114, 117, 101] => Lexeme::Value(Value::Bool(true)),
                            [102, 97, 108, 115, 101] => Lexeme::Value(Value::Bool(false)),
                            [110, 117, 108, 108] => Lexeme::Value(Value::Null),
                            _ => Lexeme::Identifier,
                        };
                        self.token = Token::Ready(token, position);
                    }
                }
                Token::StringScan {
                    start,
                    position,
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
                            self.string_reservation = JsString::reserve(heap, end - start, 1)?;
                            let mut units = Vec::new();
                            units
                                .try_reserve_exact(end - start)
                                .map_err(|_| Fault::OutOfMemory("JSON string buffer"))?;
                            self.token = Token::StringDecode {
                                position,
                                next: start,
                                end,
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
                                            return Err(self.error("Bad Unicode escape"));
                                        }
                                        self.consume();
                                    }
                                }
                                None => return Err(self.error("Unexpected end of JSON input")),
                                _ => return Err(self.error("Bad escaped character")),
                            }
                        }
                        Some(0..=31) => {
                            return Err(self.error("Bad control character in string literal"));
                        }
                        None => return Err(self.error("Unexpected end of JSON input")),
                        Some(_) => self.consume(),
                    }
                    self.token = Token::StringScan {
                        start,
                        position,
                        key,
                        decoded: decoded + 1,
                    };
                }
                Token::StringDecode {
                    position,
                    mut next,
                    end,
                    mut units,
                } => {
                    if next == end {
                        let text = JsString::from_owned_code_units(units)
                            .ok_or(Fault::StringTooLong)?
                            .with_reservation(self.string_reservation.take());
                        self.token = Token::Ready(Lexeme::String(text), position);
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
                        position,
                        next,
                        end,
                        units,
                    };
                }
                Token::NumberScan {
                    start,
                    position,
                    state,
                } => {
                    let digit = matches!(self.peek(), Some(48..=57));
                    if state == 2 && digit {
                        // QuickJS points at the leading zero, after any '-'.
                        let zero = Position {
                            offset: self.next - 1,
                            line: self.line,
                            column: self.column - 1,
                        };
                        return Err(self.error_at(zero, "Unexpected number"));
                    }
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
                        self.token = Token::NumberScan {
                            start,
                            position,
                            state,
                        };
                    } else {
                        match state {
                            0 | 1 => {
                                let c = self.peek().unwrap_or(0);
                                let message = if c == 0 {
                                    "Unexpected token '".to_string()
                                } else {
                                    // QuickJS's %c formats the first UTF-8
                                    // byte; a non-ASCII leading byte becomes
                                    // U+FFFD when its error buffer is decoded.
                                    format!(
                                        "Unexpected token '{}'",
                                        if c < 128 {
                                            char::from(c as u8)
                                        } else {
                                            '\u{fffd}'
                                        }
                                    )
                                };
                                return Err(self.error(&message));
                            }
                            4 => return Err(self.error("Unterminated fractional number")),
                            6 | 7 => return Err(self.error("Exponent part is missing a number")),
                            _ => {}
                        }
                        self.token = Token::NumberValue(
                            Box::new(NumberParse::range(
                                self.text.clone(),
                                None,
                                start,
                                self.next,
                            )),
                            position,
                        );
                    }
                }
                Token::NumberValue(mut parse, position) => {
                    if let Some(number) = parse.advance(STEP) {
                        self.token = Token::Ready(Lexeme::Value(Value::Number(number)), position);
                    } else {
                        self.token = Token::NumberValue(parse, position);
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
use crate::value::string::{CursorStep, MAX_STRING_LEN, StringBuild, UnitCursor, number_to_string};
use std::collections::HashSet;

// Own-key collection is a shared leaf API. Bound its scan/sort here; arrays
// stream their indices and have no corresponding element-count limit.
const MAX_OBJECT_PROPERTIES: usize = 4096;

pub(super) fn check_property_count(count: usize) -> Result<(), Fault> {
    if count > MAX_OBJECT_PROPERTIES {
        Err(Fault::RangeError(
            "JSON.stringify object property limit of 4096 exceeded".into(),
        ))
    } else {
        Ok(())
    }
}

/// Bound every store scanned by leaf own-key enumeration, including hidden
/// and symbol properties and virtual keys. Arrays serialized as arrays stream
/// their indices instead; this check is only for key snapshots.
pub(super) fn check_key_snapshot(heap: &Heap, task: TaskId, id: ObjectId) -> Result<(), Fault> {
    let function = heap
        .read(id, |o| {
            matches!(o, HeapObject::Function(_) | HeapObject::BoundFunction(_))
        })
        .ok_or(Fault::Internal("JSON.stringify object reclaimed"))?;
    let id = if function {
        match super::function::property_record(heap, id)? {
            Some(record) => record,
            None => return Ok(()), // At most length, name, and prototype.
        }
    } else {
        id
    };
    let typed_length = crate::object::typed_array::view(heap, task, id)?
        .map(|v| v.live_length(heap))
        .transpose()?
        .unwrap_or(0);
    heap.read(id, |o| {
        let (store, virtual_keys) = match o {
            HeapObject::Plain(o) => {
                let virtual_keys = if !o.is_intrinsic()
                    && let Some(PrimitiveData::String(s)) = o.primitive_data()
                {
                    s.len().saturating_add(1)
                } else {
                    0
                };
                (o, virtual_keys)
            }
            HeapObject::JsMap(m) | HeapObject::JsSet(m) => (m.object(), 0),
            HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => (m.object(), 0),
            HeapObject::WeakRef(m) => (m.object(), 0),
            HeapObject::FinalizationRegistry(m) => (m.object(), 0),
            HeapObject::DisposableStack(m) => (m.object(), 0),
            HeapObject::ArrayBuffer(b) => (&b.object, 0),
            HeapObject::TypedArray(a) => (&a.object, typed_length),
            HeapObject::Generator(g) => (&g.object, 0),
            HeapObject::Promise(p) => match p.properties.as_deref() {
                Some(properties) => (properties, 0),
                None => return Ok(()),
            },
            HeapObject::Builtin(f) => (&f.properties, 0),
            // A length bound also bounds scanning holes in dense storage.
            HeapObject::Array(a) => (a.object(), (a.length() as usize).saturating_add(1)),
            HeapObject::ModuleNamespace(ns) => {
                ns.check_owner(task)?;
                return check_property_count(ns.exports.len().saturating_add(1));
            }
            HeapObject::Function(_) | HeapObject::BoundFunction(_) | HeapObject::Proxy(_) => {
                return Err(Fault::Internal("JSON key snapshot store unresolved"));
            }
            HeapObject::Native(_)
            | HeapObject::Vector(_)
            | HeapObject::Map(_)
            | HeapObject::Occurrence(_)
            | HeapObject::Form(_)
            | HeapObject::Builder(_)
            | HeapObject::Cell(_)
            | HeapObject::Atom(_)
            | HeapObject::PromiseReaction(_)
            | HeapObject::PromiseResolving(_)
            | HeapObject::AsyncState(_) => return Ok(()),
        };
        if let Some(owner) = store.owner()
            && owner != task
        {
            return Err(Fault::ForeignObject {
                owner: Some(owner),
                caller: task,
            });
        }
        check_property_count(virtual_keys)?;
        check_property_count(
            virtual_keys
                + store
                    .properties()
                    .take(MAX_OBJECT_PROPERTIES - virtual_keys + 1)
                    .count(),
        )
    })
    .ok_or(Fault::Internal("JSON.stringify object reclaimed"))?
}
// StringToNumber remains a leaf; bound strings returned by a Number
// wrapper's observable ToPrimitive before invoking its scanner/parser.
pub(super) const MAX_NUMBER_CONVERSION_UNITS: usize = 4096;

#[derive(Debug)]
enum Members {
    Array {
        next: u64,
        len: u64,
    },
    Object {
        keys: std::vec::IntoIter<keys::OwnKey>,
        /// The snapshot's charge, held while the container is open.
        _charge: crate::heap::budget::ByteReservation,
        emitted: bool,
    },
}

#[derive(Debug)]
struct StringifyContainer {
    value: Value,
    members: Members,
}

#[derive(Debug)]
enum AfterIndent {
    Key,
    Emit,
    Close(bool),
}

#[derive(Debug)]
enum StringifyStage {
    ArrayLength,
    ArrayLengthValue,
    ArrayLengthNumber,
    ProxyKeys,
    Space,
    SpaceConverted,
    SpacePrefix {
        cursor: UnitCursor,
        units: Vec<u16>,
    },
    SpaceCleanup(UnitCursor),
    Prepare,
    Method,
    ToJsonResult,
    Unwrap,
    Converted,
    Ready,
    Key,
    Emit,
    Indent {
        remaining: usize,
        after: AfterIndent,
    },
    Close(bool),
    Next,
    PropertyResult,
    Quote {
        cursor: UnitCursor,
        high: Option<u16>,
        key: bool,
    },
    Build(StringBuild),
}

pub(super) enum StringifyStep {
    Native(super::native::Action),
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
    proxy_keys: Option<Box<super::proxy::State>>,
    value: Value,
    space: Value,
    gap: JsString,
    key: JsString,
    stack: Vec<StringifyContainer>,
    active: HashSet<ObjectId>,
    stage: StringifyStage,
    parts: Vec<(JsString, usize, usize)>,
    parts_reservation: crate::heap::budget::ByteReservation,
    units: usize,
}

impl Trace for Stringifier {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.value.trace(visit);
        self.space.trace(visit);
        if let Some(proxy) = &self.proxy_keys {
            proxy.trace(visit);
        }
        for container in &self.stack {
            container.value.trace(visit);
        }
    }
}

impl Stringifier {
    pub(super) fn new(value: Value, space: Value) -> Self {
        Self {
            proxy_keys: None,
            value,
            space,
            gap: JsString::from(""),
            key: JsString::from(""),
            stack: Vec::new(),
            active: HashSet::new(),
            stage: StringifyStage::Space,
            parts: Vec::new(),
            parts_reservation: Default::default(),
            units: 0,
        }
    }

    fn set_gap(&mut self) {
        self.stage = StringifyStage::Prepare;
        self.gap = match std::mem::replace(&mut self.space, Value::Undefined) {
            Value::Number(n) => {
                // Rust's saturating float cast maps NaN/negative values to zero
                // and truncates positive fractions, including after clamping.
                let count = n.clamp(0.0, 10.0) as usize;
                JsString::from_code_units(vec![32; count]).expect("gap at most 10 units")
            }
            Value::String(s) => {
                self.stage = StringifyStage::SpacePrefix {
                    cursor: UnitCursor::new(s),
                    units: Vec::with_capacity(10),
                };
                JsString::from("")
            }
            _ => JsString::from(""),
        };
    }

    fn after_indent(&mut self, after: AfterIndent) {
        self.stage = match after {
            AfterIndent::Key => StringifyStage::Key,
            AfterIndent::Emit => StringifyStage::Emit,
            AfterIndent::Close(array) => StringifyStage::Close(array),
        };
    }

    fn indent(&mut self, heap: &Heap, depth: usize, after: AfterIndent) -> Result<(), Fault> {
        if self.gap.is_empty() {
            self.after_indent(after);
        } else {
            self.append(heap, JsString::from("\n"))?;
            self.stage = StringifyStage::Indent {
                remaining: depth * self.gap.len(),
                after,
            };
        }
        Ok(())
    }

    fn append(&mut self, heap: &Heap, text: JsString) -> Result<(), Fault> {
        self.units = self
            .units
            .checked_add(text.len())
            .filter(|n| *n <= MAX_STRING_LEN)
            .ok_or(Fault::StringTooLong)?;
        if !text.is_empty() {
            let len = text.len();
            let charge = heap.reserve_bytes(128).ok_or(Fault::HeapLimit)?;
            let text = text.import(heap).ok_or(Fault::HeapLimit)?;
            self.parts.push((text, 0, len));
            self.parts_reservation.merge(charge);
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
        // IsArray follows Proxy targets without traps; SerializeJSONArray
        // subsequently reads and converts length on the original receiver.
        if builtin::is_array(heap, &self.value)? {
            self.stage = StringifyStage::ArrayLength;
            return Ok(());
        }
        if super::proxy::inspect(heap, task, id)?.is_some() {
            self.proxy_keys = Some(Box::new(super::proxy::State::json_keys(id)));
            self.stage = StringifyStage::ProxyKeys;
            return Ok(());
        }
        check_key_snapshot(heap, task, id)?;
        let (mut keys, charge) =
            keys::own_property_keys_charged(heap, task, id, keys::KeyFilter::Enumerable)?;
        keys.retain(|key| !matches!(key.key, property::Key::Symbol(_)));
        self.append(heap, JsString::from("{"))?;
        let members = Members::Object {
            keys: keys.into_iter(),
            _charge: charge,
            emitted: false,
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

    /// One transition, one property/callback, or at most 256 rope-node/unit
    /// visits or output copies. No recursive traversal or string scan.
    pub(super) fn advance(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Option<Value>,
    ) -> Result<StringifyStep, Fault> {
        use StringifyStage as S;
        use StringifyStep as A;
        match std::mem::replace(&mut self.stage, S::Next) {
            S::ArrayLength => {
                match property::read(
                    heap,
                    task,
                    &self.value,
                    &property::Key::Name(JsString::from("length")),
                )? {
                    Some(property::Read::Deferred(read)) => {
                        self.stage = S::ArrayLengthValue;
                        return Ok(A::Native(read.action(self.value.clone())));
                    }
                    read => {
                        self.stage = S::ArrayLengthNumber;
                        return Ok(A::ToNumber(match read {
                            Some(property::Read::Value(value)) => value,
                            _ => Value::Undefined,
                        }));
                    }
                }
            }
            S::ArrayLengthValue => {
                self.stage = S::ArrayLengthNumber;
                return Ok(A::ToNumber(
                    input.ok_or(Fault::Internal("JSON.stringify missing array length"))?,
                ));
            }
            S::ArrayLengthNumber => {
                let len = super::native::to_length(
                    &input.ok_or(Fault::Internal("JSON.stringify missing length conversion"))?,
                )?;
                self.append(heap, JsString::from("["))?;
                let Value::Object(id) = self.value else {
                    return Err(Fault::Internal("JSON.stringify array container"));
                };
                self.active.insert(id);
                self.stack.push(StringifyContainer {
                    value: std::mem::replace(&mut self.value, Value::Undefined),
                    members: Members::Array { next: 0, len },
                });
            }
            S::ProxyKeys => {
                let input = input
                    .map(super::native::Input::CallResult)
                    .unwrap_or(super::native::Input::Start);
                let action = self.proxy_keys.as_mut().unwrap().step(heap, task, input)?;
                if let super::native::Action::Return(array) = action {
                    let len = property::get(
                        heap,
                        task,
                        &array,
                        &property::Key::Name(JsString::from("length")),
                    )?;
                    let Value::Number(len) = len else {
                        return Err(Fault::Internal("Proxy JSON keys length"));
                    };
                    check_property_count(len as usize)?;
                    let charge = crate::heap::budget::reserve_elements(
                        heap,
                        len as usize,
                        keys::SNAPSHOT_KEY_BYTES,
                    )?;
                    let mut keys = Vec::new();
                    for i in 0..len as u32 {
                        let Value::String(name) =
                            property::get(heap, task, &array, &property::Key::Index(i))?
                        else {
                            return Err(Fault::Internal("Proxy JSON key is not a string"));
                        };
                        keys.push(keys::OwnKey {
                            key: property::Key::from_name(&name),
                            enumerable: true,
                        });
                    }
                    self.append(heap, JsString::from("{"))?;
                    let Value::Object(id) = self.value else {
                        return Err(Fault::Internal("Proxy JSON container"));
                    };
                    self.active.insert(id);
                    self.stack.push(StringifyContainer {
                        value: self.value.clone(),
                        members: Members::Object {
                            keys: keys.into_iter(),
                            _charge: charge,
                            emitted: false,
                        },
                    });
                    self.value = Value::Undefined;
                    self.proxy_keys = None;
                } else {
                    self.stage = S::ProxyKeys;
                    return Ok(A::Native(action));
                }
            }
            S::Space => {
                match builtin::wrapper_data(heap, task, &self.space)? {
                    Some(PrimitiveData::Number(_)) => {
                        self.stage = S::SpaceConverted;
                        return Ok(A::ToNumber(self.space.clone()));
                    }
                    Some(PrimitiveData::String(_)) => {
                        self.stage = S::SpaceConverted;
                        return Ok(A::ToString(self.space.clone()));
                    }
                    None
                    | Some(
                        PrimitiveData::Symbol(_)
                        | PrimitiveData::Boolean(_)
                        | PrimitiveData::BigInt(_),
                    ) => {}
                }
                self.set_gap();
            }
            S::SpaceConverted => {
                self.space =
                    input.ok_or(Fault::Internal("JSON.stringify missing space conversion"))?;
                self.set_gap();
            }
            S::SpacePrefix {
                mut cursor,
                mut units,
            } => {
                for _ in 0..STEP {
                    match cursor.step() {
                        CursorStep::Unit(unit) => units.push(unit),
                        CursorStep::Continue => continue,
                        CursorStep::End => {
                            self.gap = JsString::from_owned_code_units(units).expect("bounded gap");
                            self.stage = S::Prepare;
                            return Ok(A::Continue);
                        }
                    }
                    if units.len() == 10 {
                        self.gap = JsString::from_owned_code_units(units).expect("bounded gap");
                        self.stage = S::SpaceCleanup(cursor);
                        return Ok(A::Continue);
                    }
                }
                self.stage = S::SpacePrefix { cursor, units };
            }
            S::SpaceCleanup(mut cursor) => {
                for _ in 0..STEP {
                    if !cursor.discard_step() {
                        self.stage = S::Prepare;
                        return Ok(A::Continue);
                    }
                }
                self.stage = S::SpaceCleanup(cursor);
            }
            S::Prepare => {
                if matches!(self.value, Value::Object(_)) {
                    match property::read(
                        heap,
                        task,
                        &self.value,
                        &property::Key::Name(JsString::from("toJSON")),
                    )? {
                        Some(property::Read::Deferred(callee)) => {
                            self.stage = S::Method;
                            return Ok(StringifyStep::Native(callee.action(self.value.clone())));
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
                    Some(PrimitiveData::Boolean(value)) => self.value = Value::Bool(value),
                    Some(PrimitiveData::BigInt(value)) => {
                        self.value = Value::BigInt((*value).clone())
                    }
                    None | Some(PrimitiveData::Symbol(_)) => {}
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
                        self.append(heap, JsString::from(","))?;
                    }
                    self.indent(heap, self.stack.len(), AfterIndent::Key)?;
                } else if self.stack.is_empty() {
                    self.stage = S::Emit;
                } else {
                    self.indent(heap, self.stack.len(), AfterIndent::Emit)?;
                }
            }
            S::Key => {
                self.append(heap, JsString::from("\""))?;
                // The cursor releases the source incrementally too; do not
                // retain a redundant owner until a later native transition.
                let text = std::mem::replace(&mut self.key, JsString::from(""));
                self.stage = S::Quote {
                    cursor: UnitCursor::new(text),
                    high: None,
                    key: true,
                };
            }
            S::Indent { remaining, after } => {
                let ticket = JsString::reserve(heap, STEP, 1)?;
                let count = remaining.min(STEP);
                let gap = self.gap.code_units();
                // remaining starts at a whole number of gaps. Preserve its
                // position even when a 256-unit chunk splits a gap.
                let offset = (gap.len() - remaining % gap.len()) % gap.len();
                let units = (0..count)
                    .map(|i| gap[(offset + i) % gap.len()])
                    .collect::<Vec<_>>();
                self.append(
                    heap,
                    JsString::from_owned_code_units(units)
                        .expect("bounded indentation chunk")
                        .with_reservation(ticket),
                )?;
                if remaining == count {
                    self.after_indent(after);
                } else {
                    self.stage = S::Indent {
                        remaining: remaining - count,
                        after,
                    };
                }
            }
            S::Close(array) => self.append(heap, JsString::from(if array { "]" } else { "}" }))?,
            S::Emit => match &self.value {
                Value::String(_) => {
                    self.append(heap, JsString::from("\""))?;
                    let Value::String(text) = std::mem::replace(&mut self.value, Value::Undefined)
                    else {
                        unreachable!("emitting a string")
                    };
                    self.stage = S::Quote {
                        cursor: UnitCursor::new(text),
                        high: None,
                        key: false,
                    };
                }
                Value::Number(n) => self.append(
                    heap,
                    JsString::from(
                        if n.is_finite() {
                            number_to_string(*n)
                        } else {
                            "null".into()
                        }
                        .as_str(),
                    ),
                )?,
                Value::Null => self.append(heap, JsString::from("null"))?,
                Value::Bool(b) => {
                    self.append(heap, JsString::from(if *b { "true" } else { "false" }))?
                }
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
                    self.stage = S::Build(StringBuild::with_length_accounted(
                        heap,
                        std::mem::take(&mut self.parts),
                        self.units,
                    )?);
                    return Ok(A::Continue);
                };
                let (key, comma) = match &mut top.members {
                    Members::Array { next, len } if *next < *len => {
                        let key = super::native::index_key(*next);
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
                    let (array, emitted) = match container.members {
                        Members::Array { len, .. } => (true, len != 0),
                        Members::Object { emitted, .. } => (false, emitted),
                    };
                    if emitted {
                        self.indent(heap, self.stack.len(), AfterIndent::Close(array))?;
                    } else {
                        self.stage = S::Close(array);
                    }
                    return Ok(A::Continue);
                };
                let holder = top.value.clone();
                self.key = match &key {
                    property::Key::Index(n) => crate::vm::keys::index_name(heap, *n)?,
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
                    self.append(heap, JsString::from(","))?;
                }
                match property::read(heap, task, &holder, &key)? {
                    Some(property::Read::Deferred(callee)) => {
                        self.stage = S::PropertyResult;
                        return Ok(StringifyStep::Native(callee.action(holder)));
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
                mut cursor,
                mut high,
                key,
            } => {
                // Reserve for one saved high surrogate plus this slice.
                // Node visits count too, so a deep rope prefix must yield.
                let ticket = JsString::reserve(heap, (STEP + 1) * 6, 1)?;
                let mut units = Vec::with_capacity((STEP + 1) * 6);
                let mut done = false;
                for _ in 0..STEP {
                    let c = match cursor.step() {
                        CursorStep::Unit(c) => c,
                        CursorStep::Continue => continue,
                        CursorStep::End => {
                            if let Some(c) = high.take() {
                                quote_unit(&mut units, c);
                            }
                            done = true;
                            break;
                        }
                    };
                    if let Some(previous) = high.take() {
                        if (0xdc00..=0xdfff).contains(&c) {
                            units.extend([previous, c]);
                            continue;
                        }
                        quote_unit(&mut units, previous);
                    }
                    // Delay a high surrogate until the next unit or End,
                    // even across a rope boundary or budget exhaustion.
                    if (0xd800..=0xdbff).contains(&c) {
                        high = Some(c);
                    } else {
                        quote_unit(&mut units, c);
                    }
                }
                if !units.is_empty() {
                    self.append(
                        heap,
                        JsString::from_owned_code_units(units)
                            .expect("bounded JSON quote chunk")
                            .with_reservation(ticket),
                    )?;
                }
                if done {
                    self.append(
                        heap,
                        JsString::from(if key {
                            if self.gap.is_empty() { "\":" } else { "\": " }
                        } else {
                            "\""
                        }),
                    )?;
                    self.stage = if key { S::Emit } else { S::Next };
                } else {
                    self.stage = S::Quote { cursor, high, key };
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

// A pair is emitted together by Quote; any surrogate reaching this helper
// is unpaired and must use the well-formed JSON escape.
fn quote_unit(units: &mut Vec<u16>, c: u16) {
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
    } else if c <= 31 || (0xd800..=0xdfff).contains(&c) {
        const HEX: &[u8] = b"0123456789abcdef";
        units.extend([92, 117]);
        for shift in [12, 8, 4, 0] {
            units.push(u16::from(HEX[((c >> shift) & 15) as usize]));
        }
    } else {
        units.push(c);
    }
}

#[cfg(test)]
mod stringify_budget_tests {
    use super::*;
    use crate::value::string::cursor_release_counts;

    #[test]
    fn stringify_overflow_precedes_output_allocation() {
        let mut stringify = Stringifier::new(Value::Null, Value::Undefined);
        stringify.units = MAX_STRING_LEN;
        assert!(
            stringify
                .append(&Heap::default(), JsString::from(""))
                .is_ok()
        );
        assert!(matches!(
            stringify.append(&Heap::default(), JsString::from("x")),
            Err(Fault::StringTooLong)
        ));
        assert!(stringify.parts.is_empty());
    }

    #[test]
    fn json_stringify_rope_keys_and_surrogates_are_budgeted() {
        // Include a pair across rope children, adjacent high surrogates,
        // an unpaired low, ordinary escapes, and a final unpaired high.
        // Vary the prefix so a budget boundary can split any of them.
        for (prefix, tail) in [
            (33, 0),
            (253, 0),
            (254, 0),
            (255, 0),
            (256, 0),
            (511, 0),
            (255, 3500), // Deep prefix traversal must also yield.
        ] {
            for key in [false, true] {
                let mut left = vec![120; prefix];
                left.push(0xd83d);
                let right = JsString::from_code_units(vec![
                    0xde00, 0xd800, 0xd801, 0xdc01, 0xdc02, 10, 34, 92, 0, 0xd802,
                ])
                .unwrap();
                let mut text = JsString::from_code_units(left)
                    .unwrap()
                    .concat(&right)
                    .unwrap();
                for _ in 0..tail {
                    text = text.concat(&JsString::from("x")).unwrap();
                }
                let mut expected = vec![34];
                expected.extend(vec![120; prefix]);
                expected.extend([0xd83d, 0xde00]);
                expected.extend("\\ud800".encode_utf16());
                expected.extend([0xd801, 0xdc01]);
                expected.extend("\\udc02\\n\\\"\\\\\\u0000\\ud802".encode_utf16());
                expected.extend(vec![120; tail]);
                expected.push(34);
                if key {
                    expected.extend([58, 32]);
                }

                let mut stringify = if key {
                    let mut s = Stringifier::new(Value::Null, Value::Undefined);
                    s.key = text;
                    s.gap = JsString::from(" ");
                    s.stage = StringifyStage::Key;
                    s
                } else {
                    let mut s = Stringifier::new(Value::String(text), Value::Undefined);
                    s.stage = StringifyStage::Emit;
                    s
                };
                let heap = Heap::default();
                let mut complete = false;
                let mut steps = 0;
                for _ in 0..100 {
                    let before = cursor_release_counts();
                    let copies = JsString::copied_code_units();
                    assert!(matches!(
                        stringify.advance(&heap, TaskId::from_raw(1), None),
                        Ok(StringifyStep::Continue)
                    ));
                    let after = cursor_release_counts();
                    assert!(after.0 - before.0 <= STEP, "unbudgeted cursor walk");
                    assert!(
                        after.1 - before.1 <= STEP,
                        "unbudgeted rope release: {} bodies, prefix {prefix}, tail {tail}, key {key}, step {steps}",
                        after.1 - before.1
                    );
                    assert_eq!(JsString::copied_code_units(), copies, "rope flattened");
                    steps += 1;
                    if matches!(stringify.stage, StringifyStage::Emit) && key
                        || matches!(stringify.stage, StringifyStage::Next) && !key
                    {
                        complete = true;
                        break;
                    }
                }
                assert!(complete);
                assert!(steps > (prefix + tail) / STEP);
                let actual: Vec<_> = stringify
                    .parts
                    .iter()
                    .flat_map(|(s, _, _)| s.code_units().iter().copied())
                    .collect();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn json_parse_invalid_identifier_is_budgeted() {
        let text = JsString::from("x".repeat(65536).as_str());
        let heap = Heap::default();
        let mut parser = Parser::new(&heap, text).ok().expect("input buffer");
        for step in 0..1000 {
            match parser.advance(&heap, TaskId::from_raw(1)) {
                Ok(None) => {}
                Err(Fault::JsonSyntaxError {
                    message,
                    line,
                    column,
                }) => {
                    assert!(step >= 65536 / STEP);
                    assert_eq!((line, column), (1, 1));
                    assert_eq!(message, format!("unexpected token: '{}", "x".repeat(236)));
                    assert_eq!(message.len(), 255);
                    return;
                }
                _ => panic!("expected an invalid identifier"),
            }
        }
        panic!("JSON parser did not finish");
    }

    #[test]
    fn json_parse_rope_error_position_work_is_budgeted() {
        let mut text = JsString::from(" ".repeat(1024).as_str());
        for _ in 0..2048 {
            text = text.concat(&JsString::from("\r\n")).unwrap();
        }
        let suffix = JsString::from_code_units(vec![
            91, 34, 0xd83d, 0xde00, 0xd800, 34, 44, 116, 114, 117, 93,
        ])
        .unwrap();
        text = text.concat(&suffix).unwrap();
        let heap = Heap::default();
        let mut parser = Parser::new(&heap, text).ok().expect("input buffer");
        let mut steps = 0;
        for _ in 0..1000 {
            let copies = JsString::copied_code_units();
            let result = parser.advance(&heap, TaskId::from_raw(1));
            assert!(
                JsString::copied_code_units() - copies <= STEP as u64,
                "a parser step flattened the rope synchronously"
            );
            steps += 1;
            match result {
                Ok(None) => {}
                Err(Fault::JsonSyntaxError {
                    message,
                    line,
                    column,
                }) => {
                    assert_eq!((line, column), (2049, 8));
                    assert_eq!(message, "unexpected token: 'tru'");
                    assert!(steps >= 40);
                    return;
                }
                _ => panic!("expected a positioned JSON error"),
            }
        }
        panic!("JSON parser did not finish");
    }

    #[test]
    fn json_stringify_space_prefix_cleanup_is_budgeted() {
        for deep_suffix in [false, true] {
            let mut text = JsString::from("abcdefghijklmnopqrstuvwxyz0123456789");
            for _ in 0..10000 {
                text = text.concat(&JsString::from("x")).unwrap();
            }
            if deep_suffix {
                // After taking ten units, cleanup owns an unvisited deep
                // subtree, rather than just a list of flat right siblings.
                text = JsString::from("abcdefghijklmnopqrstuvwxyz0123456789")
                    .concat(&text)
                    .unwrap();
            }
            let heap = Heap::default();
            let mut stringify = Stringifier::new(Value::Number(1.0), Value::String(text));
            let mut cleanup_steps = 0;
            let mut complete = false;
            for _ in 0..1000 {
                let before = cursor_release_counts();
                if matches!(stringify.stage, StringifyStage::SpaceCleanup(_)) {
                    cleanup_steps += 1;
                }
                let result = stringify
                    .advance(&heap, TaskId::from_raw(1), None)
                    .unwrap_or_else(|_| panic!("stringify failed"));
                let after = cursor_release_counts();
                assert!(
                    after.0 - before.0 <= STEP,
                    "cursor list disposal exceeded budget"
                );
                assert!(
                    after.1 - before.1 <= STEP + 16,
                    "owned rope disposal exceeded budget"
                );
                match result {
                    StringifyStep::Continue => {}
                    StringifyStep::Return(Value::String(s)) => {
                        assert_eq!(s, "1");
                        assert_eq!(stringify.gap, "abcdefghij");
                        complete = true;
                        break;
                    }
                    _ => panic!("unexpected callback or result"),
                }
            }
            assert!(complete);
            assert!(cleanup_steps >= 10000 / STEP, "cleanup must yield");
        }
    }
}
