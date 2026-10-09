//! Literal string operations on UTF-16 units, with no regex or host I/O engine.
use super::*;
use collection_core::{Cursor, truthy};
use printing::Text;

fn string(v: &RootedValue, argument: usize) -> Result<Rc<[u16]>, NativeError> {
    match v.inspect() {
        ValueView::String(s) => Ok(s),
        _ => Err(NativeError::Type {
            argument,
            expected: "string",
        }),
    }
}
fn index(v: &RootedValue, argument: usize) -> Result<i32, NativeError> {
    // Match JVM Number.intValue: truncate fractions, narrow integer low bits.
    match v.inspect() {
        ValueView::Integer(n) => Ok(n as i32),
        ValueView::Double(n) => Ok(n as i32),
        ValueView::Ratio {
            numerator,
            denominator,
        } => Ok((numerator / denominator) as i32),
        _ => Err(NativeError::Type {
            argument,
            expected: "numeric index",
        }),
    }
}
fn result(c: &Context, units: &[u16]) -> NativeResult {
    c.accounting.work(units.len() as u64)?;
    c.charge_allocation(units.len() as u64 + 1)?;
    let id = HeapId(
        c.owner
            .store
            .borrow_mut()
            .heap
            .insert(HeapValue::String(Rc::from(units))),
    );
    Ok(c.owner.checked_value(Value::Heap(id))?)
}
fn subs(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let s = string(&a[0], 0)?;
    let start = index(&a[1], 1)?;
    let end = if a.len() == 3 {
        i64::from(index(&a[2], 2)?)
    } else {
        s.len() as i64
    };
    if start < 0 || end < i64::from(start) || end > s.len() as i64 {
        return Err(NativeError::StringBounds {
            start: i64::from(start),
            end,
            length: s.len(),
        });
    }
    result(c, &s[start as usize..end as usize])
}
fn name(c: &mut Context, a: &[RootedValue], namespace: bool) -> NativeResult {
    match a[0].inspect() {
        ValueView::Symbol(n) | ValueView::Keyword(n) => {
            if namespace {
                n.namespace().map_or_else(|| Ok(c.nil()), |s| result(c, s))
            } else {
                result(c, n.name())
            }
        }
        ValueView::String(_) if !namespace => Ok(a[0].clone()),
        _ => Err(NativeError::Type {
            argument: 0,
            expected: if namespace {
                "symbol or keyword"
            } else {
                "string, symbol or keyword"
            },
        }),
    }
}
pub(super) fn split_name(units: &[u16]) -> Name {
    if units.len() > 1
        && let Some(i) = units.iter().position(|u| *u == 47)
    {
        Name::from_utf16(Some(&units[..i]), &units[i + 1..])
    } else {
        Name::from_utf16(None, units)
    }
}
fn named(c: &mut Context, a: &[RootedValue], keyword: bool) -> NativeResult {
    let n = if a.len() == 2 {
        let ns = if matches!(a[0].inspect(), ValueView::Nil) {
            None
        } else {
            Some(string(&a[0], 0)?)
        };
        let tail = string(&a[1], 1)?;
        c.charge_allocation(ns.as_ref().map_or(0, |s| s.len()) as u64 + tail.len() as u64)?;
        Name::from_utf16(ns.as_deref(), &tail)
    } else {
        match a[0].inspect() {
            ValueView::Keyword(_) if keyword => return Ok(a[0].clone()),
            ValueView::Symbol(_) if !keyword => return Ok(a[0].clone()),
            ValueView::Symbol(n) | ValueView::Keyword(n) => n,
            ValueView::Var(n) if !keyword => n,
            ValueView::String(s) => {
                c.accounting.work(s.len() as u64)?;
                c.charge_allocation(s.len() as u64)?;
                split_name(&s)
            }
            _ if keyword => return Ok(c.nil()),
            _ => {
                return Err(NativeError::Type {
                    argument: 0,
                    expected: "string, symbol or Var",
                });
            }
        }
    };
    Ok(if keyword { c.keyword(n) } else { c.symbol(n) })
}
fn gensym(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let prefix: Rc<[u16]> = if a.is_empty() {
        "G__".encode_utf16().collect()
    } else {
        string(&printing::stringify(c, &a[0])?, 0)?
    };
    let number = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| NativeError::Host("gensym counter exhausted".into()))?;
    let mut text = Text::new(false);
    text.append(c, &prefix)?;
    text.ascii(c, &number.to_string())?;
    Ok(c.symbol(split_name(&text.units)))
}
fn join(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let separator = if a.len() == 2 { Some(&a[0]) } else { None };
    let mut cursor = Cursor::new(c, &a[a.len() - 1], a.len() - 1)?;
    let mut text = Text::new(false);
    let mut first = true;
    while let Some(v) = cursor.next(c)? {
        if !first && let Some(s) = &separator {
            let printed = printing::stringify(c, s)?;
            text.append(c, &string(&printed, 0)?)?;
        }
        first = false;
        let s = printing::stringify(c, &v)?;
        text.append(c, &string(&s, 0)?)?;
    }
    text.finish(c)
}
fn split_lines(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let s = string(&a[0], 0)?;
    let mut fields = Vec::new();
    let mut start = 0;
    for (i, &unit) in s.iter().enumerate() {
        c.checkpoint(PollPoint::Iteration)?;
        if unit == 10 {
            let end = if i > start && s[i - 1] == 13 {
                i - 1
            } else {
                i
            };
            c.charge_allocation(1)?;
            fields.push(result(c, &s[start..end])?);
            start = i + 1;
        }
    }
    fields.push(result(c, &s[start..])?);
    if !s.is_empty() {
        while fields
            .last()
            .is_some_and(|v| matches!(v.inspect(), ValueView::String(s) if s.is_empty()))
        {
            fields.pop();
        }
    }
    Ok(c.vector(&fields)?)
}
fn whitespace(u: u16) -> bool {
    matches!(u, 9..=13 | 0x1c..=0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200a | 0x2028..=0x2029 | 0x205f | 0x3000)
}
fn trim(c: &mut Context, a: &[RootedValue], left: bool, right: bool) -> NativeResult {
    let s = string(&a[0], 0)?;
    let mut start = 0;
    let mut end = s.len();
    if left {
        while start < end && whitespace(s[start]) {
            c.checkpoint(PollPoint::Iteration)?;
            start += 1;
        }
    }
    if right {
        while start < end && whitespace(s[end - 1]) {
            c.checkpoint(PollPoint::Iteration)?;
            end -= 1;
        }
    }
    result(c, &s[start..end])
}
fn blank(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    if matches!(a[0].inspect(), ValueView::Nil) {
        return Ok(c.boolean(true));
    }
    let s = string(&a[0], 0)?;
    for &u in s.iter() {
        c.checkpoint(PollPoint::Iteration)?;
        if !whitespace(u) {
            return Ok(c.boolean(false));
        }
    }
    Ok(c.boolean(true))
}
fn search(
    c: &Context,
    s: &[u16],
    needle: &[u16],
    start: usize,
    last: bool,
) -> Result<Option<usize>, NativeError> {
    if needle.is_empty() {
        return Ok(Some(start.min(s.len())));
    }
    if needle.len() > s.len() {
        return Ok(None);
    }
    let end = s.len() - needle.len();
    let (lo, hi) = if last {
        (0, start.min(end))
    } else if start <= end {
        (start, end)
    } else {
        return Ok(None);
    };
    for offset in 0..=hi - lo {
        c.checkpoint(PollPoint::Iteration)?;
        c.accounting.work(needle.len() as u64)?;
        let i = if last { hi - offset } else { lo + offset };
        if s[i..i + needle.len()] == *needle {
            return Ok(Some(i));
        }
    }
    Ok(None)
}
fn index_of(c: &mut Context, a: &[RootedValue], last: bool) -> NativeResult {
    let s = string(&a[0], 0)?;
    let needle = match a[1].inspect() {
        ValueView::Character(u) => Rc::from([u]),
        _ => string(&a[1], 1)?,
    };
    let start = if a.len() == 3 {
        i64::from(index(&a[2], 2)?)
    } else if last {
        s.len() as i64
    } else {
        0
    };
    if last && start < 0 {
        return Ok(c.nil());
    }
    let found = search(c, &s, &needle, start.max(0) as usize, last)?;
    Ok(found.map_or_else(|| c.nil(), |i| c.integer(i as i64)))
}
fn contains(c: &mut Context, a: &[RootedValue], kind: u8) -> NativeResult {
    let s = string(&a[0], 0)?;
    let needle = string(&a[1], 1)?;
    c.accounting.work(needle.len() as u64)?;
    let matched = match kind {
        0 => s.starts_with(&needle),
        1 => s.ends_with(&needle),
        _ => search(c, &s, &needle, 0, false)?.is_some(),
    };
    Ok(c.boolean(matched))
}
fn replace(c: &mut Context, a: &[RootedValue], first: bool) -> NativeResult {
    let s = string(&a[0], 0)?;
    let (needle, replacement) = match a[1].inspect() {
        ValueView::Character(u) => {
            let ValueView::Character(v) = a[2].inspect() else {
                return Err(NativeError::Type {
                    argument: 2,
                    expected: "character replacement",
                });
            };
            (Rc::from([u]), Rc::from([v]))
        }
        _ => (string(&a[1], 1)?, string(&a[2], 2)?),
    };
    let mut text = Text::new(false);
    let mut position = 0;
    loop {
        let Some(i) = search(c, &s, &needle, position, false)? else {
            text.append(c, &s[position..])?;
            break;
        };
        text.append(c, &s[position..i])?;
        text.append(c, &replacement)?;
        position = i + needle.len();
        if first {
            text.append(c, &s[position..])?;
            break;
        }
        if needle.is_empty() {
            if position == s.len() {
                break;
            }
            text.append(c, &s[position..position + 1])?;
            position += 1;
        }
    }
    text.finish(c)
}
fn case_units(c: &Context, text: &mut Text, s: &[u16], upper: bool) -> Result<(), NativeError> {
    for decoded in char::decode_utf16(s.iter().copied()) {
        c.checkpoint(PollPoint::Iteration)?;
        match decoded {
            Ok(ch) => {
                if upper {
                    for ch in ch.to_uppercase() {
                        text.append(c, ch.encode_utf16(&mut [0; 2]))?;
                    }
                } else {
                    for ch in ch.to_lowercase() {
                        text.append(c, ch.encode_utf16(&mut [0; 2]))?;
                    }
                }
            }
            Err(error) => text.append(c, &[error.unpaired_surrogate()])?,
        }
    }
    Ok(())
}
fn change_case(c: &mut Context, a: &[RootedValue], kind: u8) -> NativeResult {
    if matches!(a[0].inspect(), ValueView::Nil) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "non-nil value",
        });
    }
    let coerced = printing::stringify(c, &a[0])?;
    let s = string(&coerced, 0)?;
    let mut text = Text::new(false);
    if kind == 2 && !s.is_empty() {
        case_units(c, &mut text, &s[..1], true)?;
        case_units(c, &mut text, &s[1..], false)?;
    } else {
        case_units(c, &mut text, &s, kind == 0)?;
    }
    text.finish(c)
}
fn reverse(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let s = string(&a[0], 0)?;
    let mut text = Text::new(false);
    let mut i = s.len();
    while i != 0 {
        i -= 1;
        if (0xdc00..=0xdfff).contains(&s[i]) && i > 0 && (0xd800..=0xdbff).contains(&s[i - 1]) {
            text.append(c, &s[i - 1..=i])?;
            i -= 1;
        } else {
            text.append(c, &s[i..=i])?;
        }
    }
    text.finish(c)
}
fn escape(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let s = string(&a[0], 0)?;
    let mut text = Text::new(false);
    for &u in s.iter() {
        c.checkpoint(PollPoint::Iteration)?;
        let replacement = c.invoke(&a[1], &[c.character(u)])?;
        if truthy(&replacement) {
            let printed = printing::stringify(c, &replacement)?;
            text.append(c, &string(&printed, 0)?)?;
        } else {
            text.append(c, &[u])?;
        }
    }
    text.finish(c)
}
macro_rules! def {
    ($ns:literal, $name:literal, $arity:expr, $callback:expr) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: $ns,
                name: $name,
                arity: $arity,
                effects: NativeEffects::Context,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Bounds,
                    NativeErrorClass::Type,
                    NativeErrorClass::Host,
                    NativeErrorClass::Invocation,
                ],
            },
            callback: $callback,
        }
    };
}
pub fn string_core_definitions() -> &'static [NativeDefinition] {
    static DEFINITIONS: &[NativeDefinition] = &[
        def!(
            "clojure.core",
            "subs",
            Arity::Range { min: 2, max: 3 },
            subs
        ),
        def!("clojure.core", "name", Arity::Exact(1), |c, a| name(
            c, a, false
        )),
        def!("clojure.core", "namespace", Arity::Exact(1), |c, a| name(
            c, a, true
        )),
        def!(
            "clojure.core",
            "symbol",
            Arity::Range { min: 1, max: 2 },
            |c, a| named(c, a, false)
        ),
        def!(
            "clojure.core",
            "keyword",
            Arity::Range { min: 1, max: 2 },
            |c, a| named(c, a, true)
        ),
        def!(
            "clojure.core",
            "gensym",
            Arity::Range { min: 0, max: 1 },
            gensym
        ),
    ];
    DEFINITIONS
}
pub fn string_definitions() -> &'static [NativeDefinition] {
    static DEFINITIONS: &[NativeDefinition] = &[
        def!(
            "clojure.string",
            "join",
            Arity::Range { min: 1, max: 2 },
            join
        ),
        def!(
            "clojure.string",
            "split-lines",
            Arity::Exact(1),
            split_lines
        ),
        def!("clojure.string", "trim", Arity::Exact(1), |c, a| trim(
            c, a, true, true
        )),
        def!("clojure.string", "triml", Arity::Exact(1), |c, a| trim(
            c, a, true, false
        )),
        def!("clojure.string", "trim-newline", Arity::Exact(1), |c, a| {
            let s = string(&a[0], 0)?;
            let mut end = s.len();
            while end > 0 && matches!(s[end - 1], 10 | 13) {
                c.checkpoint(PollPoint::Iteration)?;
                end -= 1;
            }
            result(c, &s[..end])
        }),
        def!("clojure.string", "trimr", Arity::Exact(1), |c, a| trim(
            c, a, false, true
        )),
        def!("clojure.string", "blank?", Arity::Exact(1), blank),
        def!("clojure.string", "starts-with?", Arity::Exact(2), |c, a| {
            contains(c, a, 0)
        }),
        def!("clojure.string", "ends-with?", Arity::Exact(2), |c, a| {
            contains(c, a, 1)
        }),
        def!("clojure.string", "includes?", Arity::Exact(2), |c, a| {
            contains(c, a, 2)
        }),
        def!(
            "clojure.string",
            "index-of",
            Arity::Range { min: 2, max: 3 },
            |c, a| index_of(c, a, false)
        ),
        def!(
            "clojure.string",
            "last-index-of",
            Arity::Range { min: 2, max: 3 },
            |c, a| index_of(c, a, true)
        ),
        def!(
            "clojure.string",
            "replace",
            Arity::Exact(3),
            |c, a| replace(c, a, false)
        ),
        def!(
            "clojure.string",
            "replace-first",
            Arity::Exact(3),
            |c, a| replace(c, a, true)
        ),
        def!("clojure.string", "upper-case", Arity::Exact(1), |c, a| {
            change_case(c, a, 0)
        }),
        def!("clojure.string", "lower-case", Arity::Exact(1), |c, a| {
            change_case(c, a, 1)
        }),
        def!("clojure.string", "capitalize", Arity::Exact(1), |c, a| {
            change_case(c, a, 2)
        }),
        def!("clojure.string", "reverse", Arity::Exact(1), reverse),
        def!("clojure.string", "escape", Arity::Exact(2), escape),
    ];
    DEFINITIONS
}
