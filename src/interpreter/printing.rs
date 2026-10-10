//! Shared UTF-16 printer. No store borrow survives lazy evaluation or host calls.
use super::*;
use collection_core::{Cursor, truthy};
use std::collections::HashSet;

/// Printing limits are per collection/depth, in addition to common host budgets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrintOptions {
    pub length: Option<usize>,
    pub level: Option<usize>,
    pub metadata: bool,
    pub readably: bool,
}
impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            length: None,
            level: None,
            metadata: false,
            readably: true,
        }
    }
}

/// Reserve before growing temporary buffers. Output is counted in UTF-16 bytes,
/// including unmatched surrogate units; no UTF-8 substitution is involved.
pub(super) struct Text {
    pub(super) units: Vec<u16>,
    output: bool,
}
impl Text {
    pub(super) fn new(output: bool) -> Self {
        Self {
            units: Vec::new(),
            output,
        }
    }
    pub(super) fn append(&mut self, c: &Context, units: &[u16]) -> Result<(), NativeError> {
        c.checkpoint(PollPoint::Iteration)?;
        c.accounting.work(units.len() as u64)?;
        c.charge_allocation(units.len() as u64)?;
        if self.output {
            c.charge_output((units.len() as u64).saturating_mul(2))?;
        }
        self.units.extend_from_slice(units);
        Ok(())
    }
    pub(super) fn ascii(&mut self, c: &Context, text: &str) -> Result<(), NativeError> {
        // All callers supply small ASCII tokens, numeric spellings or escapes.
        for byte in text.bytes() {
            self.append(c, &[u16::from(byte)])?;
        }
        Ok(())
    }
    pub(super) fn finish(self, c: &Context) -> NativeResult {
        c.charge_allocation(self.units.len() as u64 + 1)?;
        let id = HeapId(
            c.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::String(self.units.into())),
        );
        Ok(c.owner.checked_value(Value::Heap(id))?)
    }
}

fn control(c: &Context, name: &str) -> Result<RootedValue, NativeError> {
    let id = c.core_var(&Name::new(None, name));
    c.accounting.check()?;
    Ok(c.var_get(id.expect("printer dynamic Var"))
        .expect("printer Var belongs to context"))
}
fn limit(v: RootedValue, length: bool) -> Result<Option<usize>, NativeError> {
    let number = match v.inspect() {
        ValueView::Nil | ValueView::Boolean(false) => return Ok(None),
        ValueView::Integer(n) => {
            return Ok(if length && n < 0 {
                None
            } else {
                Some(usize::try_from(n).unwrap_or(0))
            });
        }
        ValueView::Double(n) => n,
        ValueView::Ratio {
            numerator,
            denominator,
        } => {
            if length {
                return Ok(None);
            }
            return Ok(Some(usize::try_from(numerator / denominator).unwrap_or(0)));
        }
        _ => {
            return Err(NativeError::Type {
                argument: 0,
                expected: "numeric, nil or false print limit",
            });
        }
    };
    Ok(
        if number.is_nan() || (length && (number < 0.0 || number.fract() != 0.0)) {
            None
        } else {
            Some(number as usize)
        },
    )
}

impl PrintOptions {
    pub(super) fn dynamic(c: &Context) -> Result<Self, NativeError> {
        Ok(Self {
            length: limit(control(c, "*print-length*")?, true)?,
            level: limit(control(c, "*print-level*")?, false)?,
            metadata: truthy(&control(c, "*print-meta*")?),
            readably: truthy(&control(c, "*print-readably*")?),
        })
    }
}

struct Printer {
    text: Text,
    options: PrintOptions,
    path: HashSet<HeapId>,
    recursion: usize,
}
impl Printer {
    fn name(&mut self, c: &Context, n: &Name) -> Result<(), NativeError> {
        if let Some(ns) = n.namespace() {
            self.text.append(c, ns)?;
            self.text.ascii(c, "/")?;
        }
        self.text.append(c, n.name())
    }
    fn quoted(&mut self, c: &Context, units: &[u16]) -> Result<(), NativeError> {
        if !self.options.readably {
            return self.text.append(c, units);
        }
        self.text.ascii(c, "\"")?;
        let mut i = 0;
        while i < units.len() {
            let u = units[i];
            if (0xd800..=0xdbff).contains(&u)
                && units
                    .get(i + 1)
                    .is_some_and(|v| (0xdc00..=0xdfff).contains(v))
            {
                self.text.append(c, &units[i..i + 2])?;
                i += 2;
                continue;
            }
            i += 1;
            match u {
                8 => self.text.ascii(c, "\\b")?,
                9 => self.text.ascii(c, "\\t")?,
                10 => self.text.ascii(c, "\\n")?,
                12 => self.text.ascii(c, "\\f")?,
                13 => self.text.ascii(c, "\\r")?,
                34 => self.text.ascii(c, "\\\"")?,
                92 => self.text.ascii(c, "\\\\")?,
                0..=31 | 0x7f | 0xd800..=0xdfff => self.text.ascii(c, &format!("\\u{u:04x}"))?,
                _ => self.text.append(c, &[u])?,
            }
        }
        self.text.ascii(c, "\"")
    }
    fn value(
        &mut self,
        c: &mut Context,
        value: &RootedValue,
        depth: usize,
        raw: bool,
    ) -> Result<(), NativeError> {
        c.checkpoint(PollPoint::Iteration)?;
        c.accounting.work(1)?;
        let view = value.inspect();
        if raw {
            match &view {
                ValueView::Nil => return Ok(()),
                ValueView::String(s) => return self.text.append(c, s),
                ValueView::Character(u) => return self.text.append(c, &[*u]),
                _ => (),
            }
        }
        let collection = matches!(
            view,
            ValueView::List(_)
                | ValueView::Vector(_)
                | ValueView::Seq(_)
                | ValueView::LazySeq
                | ValueView::Set(_)
                | ValueView::Map(_)
                | ValueView::Record(_)
        );
        if self.recursion >= c.accounting.depths().1 {
            return Err(c
                .accounting
                .fail(ResourceError::EvaluationDepthExceeded)
                .into());
        }
        let id = value.heap_id();
        if let Some(id) = id {
            if self.path.contains(&id) {
                return self.text.ascii(c, "#<cycle>");
            }
            c.charge_allocation(1)?;
            self.path.insert(id);
        }
        self.recursion += 1;
        let result = (|| {
            if self.options.metadata
                && self.options.readably
                && !raw
                && let Some(meta) = value.metadata()
                && !matches!(meta.inspect(), ValueView::Nil | ValueView::Map(0))
                && !matches!(view, ValueView::Var(_) | ValueView::Atom)
            {
                self.text.ascii(c, "^")?;
                let tag = if matches!(meta.inspect(), ValueView::Map(1)) {
                    meta.get(&c.keyword(Name::new(None, "tag")))?.filter(truthy)
                } else {
                    None
                };
                self.value(c, tag.as_ref().unwrap_or(&meta), depth, false)?;
                self.text.ascii(c, " ")?;
            }
            if collection && self.options.level.is_some_and(|n| depth >= n) {
                return self.text.ascii(c, "#");
            }
            match view {
                ValueView::Nil => self.text.ascii(c, "nil"),
                ValueView::Boolean(v) => self.text.ascii(c, if v { "true" } else { "false" }),
                ValueView::Integer(v) => self.text.ascii(c, &v.to_string()),
                ValueView::Double(v) => {
                    let s = if v.is_nan() {
                        "##NaN".into()
                    } else if v == f64::INFINITY {
                        "##Inf".into()
                    } else if v == f64::NEG_INFINITY {
                        "##-Inf".into()
                    } else {
                        format!("{v:?}")
                    };
                    self.text.ascii(c, &s)
                }
                ValueView::Ratio {
                    numerator,
                    denominator,
                } => self.text.ascii(c, &format!("{numerator}/{denominator}")),
                ValueView::String(s) => self.quoted(c, &s),
                ValueView::Character(u) => {
                    if !self.options.readably {
                        return self.text.append(c, &[u]);
                    }
                    let named = match u {
                        8 => Some("backspace"),
                        9 => Some("tab"),
                        10 => Some("newline"),
                        12 => Some("formfeed"),
                        13 => Some("return"),
                        32 => Some("space"),
                        _ => None,
                    };
                    self.text.ascii(c, "\\")?;
                    if let Some(n) = named {
                        self.text.ascii(c, n)
                    } else if u < 32 || u == 127 || (0xd800..=0xdfff).contains(&u) {
                        self.text.ascii(c, &format!("u{u:04x}"))
                    } else {
                        self.text.append(c, &[u])
                    }
                }
                ValueView::Symbol(n) => self.name(c, &n),
                ValueView::Keyword(n) => {
                    self.text.ascii(c, ":")?;
                    self.name(c, &n)
                }
                ValueView::Namespace(n) => {
                    self.text.ascii(c, "#<Namespace ")?;
                    self.name(c, &n)?;
                    self.text.ascii(c, ">")
                }
                ValueView::Var(n) => {
                    self.text.ascii(c, "#'")?;
                    self.name(c, &n)
                }
                ValueView::Unbound(n) => {
                    self.text.ascii(c, "#<Unbound ")?;
                    self.name(c, &n)?;
                    self.text.ascii(c, ">")
                }
                ValueView::NativeFunction(n) => {
                    self.text.ascii(c, "#<Function ")?;
                    self.name(c, &n.qualified_name())?;
                    self.text.ascii(c, ">")
                }
                ValueView::Protocol(name) => {
                    self.text.ascii(c, "#<Protocol ")?;
                    self.name(c, &name)?;
                    self.text.ascii(c, ">")
                }
                ValueView::Reify => self.text.ascii(c, "#<Reify>"),
                ValueView::RecordType(name) => {
                    self.text.ascii(c, "#<RecordType ")?;
                    self.name(c, &name)?;
                    self.text.ascii(c, ">")
                }
                ValueView::Record(_) => {
                    let name = {
                        let store = c.owner.store.borrow();
                        records::descriptor(&store, records::type_id(value).unwrap())
                            .name
                            .clone()
                    };
                    self.text.ascii(c, "#")?;
                    if let Some(ns) = name.namespace() {
                        self.text.append(c, ns)?;
                        self.text.ascii(c, ".")?;
                    }
                    self.text.append(c, name.name())?;
                    self.hashed(c, value, depth, true)
                }
                ValueView::Function => self.text.ascii(c, "#<Function>"),
                ValueView::Input => self.text.ascii(c, "#<Input>"),
                ValueView::Output => self.text.ascii(c, "#<Output>"),
                ValueView::CompilerLiteral(_) => self.text.ascii(c, "#<compiler-reader-value>"),
                ValueView::Delay => self.text.ascii(
                    c,
                    &format!("#<Delay {:?}>", value.deferred_status().unwrap()),
                ),
                ValueView::Atom | ValueView::Volatile | ValueView::Reduced => {
                    self.text.ascii(
                        c,
                        match view {
                            ValueView::Atom => "#<Atom ",
                            ValueView::Volatile => "#<Volatile ",
                            _ => "#<Reduced ",
                        },
                    )?;
                    let state = value
                        .state_value()
                        .or_else(|| value.reduced_value())
                        .unwrap();
                    self.value(c, &state, depth + 1, false)?;
                    self.text.ascii(c, ">")
                }
                ValueView::Exception(category) => {
                    self.text.ascii(c, &format!("#<{category:?} "))?;
                    self.value(c, &value.exception_message().unwrap(), depth + 1, false)?;
                    self.text.ascii(c, " ")?;
                    self.value(c, &value.exception_data().unwrap(), depth + 1, false)?;
                    self.text.ascii(c, ">")
                }
                ValueView::Map(_) | ValueView::Set(_) => {
                    self.hashed(c, value, depth, matches!(view, ValueView::Map(_)))
                }
                ValueView::Vector(n) => {
                    self.text.ascii(c, "[")?;
                    let count = self.options.length.unwrap_or(n).min(n);
                    for i in 0..count {
                        if i != 0 {
                            self.text.ascii(c, " ")?;
                        }
                        self.value(c, &value.nth(i)?.unwrap(), depth + 1, false)?;
                    }
                    if count < n {
                        if count != 0 {
                            self.text.ascii(c, " ")?;
                        }
                        self.text.ascii(c, "...")?;
                    }
                    self.text.ascii(c, "]")
                }
                ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq => {
                    self.text.ascii(c, "(")?;
                    let mut cursor = Cursor::new(c, value, 0)?;
                    let mut count = 0;
                    while let Some(member) = cursor.next(c)? {
                        if count != 0 {
                            self.text.ascii(c, " ")?;
                        }
                        if self.options.length.is_some_and(|n| count >= n) {
                            self.text.ascii(c, "...")?;
                            break;
                        }
                        self.value(c, &member, depth + 1, false)?;
                        count += 1;
                    }
                    self.text.ascii(c, ")")
                }
            }
        })();
        self.recursion -= 1;
        if let Some(id) = id {
            self.path.remove(&id);
        }
        result
    }
    fn hashed(
        &mut self,
        c: &mut Context,
        value: &RootedValue,
        depth: usize,
        map: bool,
    ) -> Result<(), NativeError> {
        // Snapshot only the printed prefix; root every edge before callbacks.
        let (entries, more) = {
            let store = c.owner.store.borrow();
            let values = match store.heap_value(value.value()).unwrap() {
                HeapValue::Map(m) | HeapValue::Set(m) => m,
                HeapValue::Record(r) => &r.data,
                _ => unreachable!(),
            };
            let count = self
                .options
                .length
                .unwrap_or(values.len())
                .min(values.len());
            c.charge_allocation((count as u64).saturating_mul(2))?;
            c.accounting.work(count as u64)?;
            let entries: Vec<_> = values
                .entries()
                .take(count)
                .map(|(k, v)| (c.owner.value(k), c.owner.value(v)))
                .collect();
            (entries, count < values.len())
        };
        self.text.ascii(c, if map { "{" } else { "#{" })?;
        for (i, (key, val)) in entries.iter().enumerate() {
            if i != 0 {
                self.text.ascii(c, if map { ", " } else { " " })?;
            }
            self.value(c, key, depth + 1, false)?;
            if map {
                self.text.ascii(c, " ")?;
                self.value(c, val, depth + 1, false)?;
            }
        }
        if more {
            if !entries.is_empty() {
                self.text.ascii(c, if map { ", " } else { " " })?;
            }
            self.text.ascii(c, "...")?;
        }
        self.text.ascii(c, "}")
    }
}
impl Context {
    /// Format data as exact UTF-16 with the common fuel/allocation/output budget.
    /// Lazy sequences execute in this context. Identity objects use opaque markers.
    pub fn print_value(&mut self, value: &RootedValue, options: PrintOptions) -> NativeResult {
        let _scope = self.accounting.enter();
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        let result = render(self, std::slice::from_ref(value), options, false, false);
        self.accounting.check()?;
        result
    }
}
pub(super) fn render(
    c: &mut Context,
    values: &[RootedValue],
    options: PrintOptions,
    raw: bool,
    newline: bool,
) -> NativeResult {
    let mut p = Printer {
        text: Text::new(true),
        options,
        path: HashSet::new(),
        recursion: 0,
    };
    for (i, v) in values.iter().enumerate() {
        if !raw && i != 0 {
            p.text.ascii(c, " ")?;
        }
        p.value(c, v, 0, raw)?;
    }
    if newline {
        p.text.ascii(c, "\n")?;
    }
    p.text.finish(c)
}
pub(super) fn stringify(c: &mut Context, value: &RootedValue) -> NativeResult {
    let options = PrintOptions::default();
    render(c, std::slice::from_ref(value), options, true, false)
}
pub fn printing_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $body:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: Arity::AtLeast(0),
                    effects: NativeEffects::Context,
                    errors: &[NativeErrorClass::Type, NativeErrorClass::Invocation],
                },
                callback: $body,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("pr-str", |c, a| render(
            c,
            a,
            PrintOptions::dynamic(c)?,
            false,
            false
        )),
        def!("prn-str", |c, a| render(
            c,
            a,
            PrintOptions::dynamic(c)?,
            false,
            true
        )),
        def!("str", |c, a| {
            let options = PrintOptions::default();
            render(c, a, options, true, false)
        }),
    ];
    DEFINITIONS
}
