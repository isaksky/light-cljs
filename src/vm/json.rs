//! Budgeted JSON grammar parser (clj-quickjs-159). All container state is
//! explicit and traced; no Rust recursion or JavaScript property callbacks.
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
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
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
                Token::StringScan { start, key } => {
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
                    self.token = Token::StringScan { start, key };
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
