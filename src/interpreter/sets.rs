//! Persistent set algebra and indexed relational operations.
use super::*;
use collection_core::{Cursor, truthy};

fn set_count(value: &RootedValue, argument: usize) -> Result<usize, NativeError> {
    match value.inspect() {
        ValueView::Set(n) => Ok(n),
        ValueView::Nil => Ok(0),
        _ => Err(NativeError::Type {
            argument,
            expected: "set or nil",
        }),
    }
}
fn map(value: &RootedValue, argument: usize) -> Result<(), NativeError> {
    if matches!(value.inspect(), ValueView::Map(_) | ValueView::Nil) {
        Ok(())
    } else {
        Err(NativeError::Type {
            argument,
            expected: "map or nil",
        })
    }
}
fn contains(set: &RootedValue, value: &RootedValue) -> Result<bool, NativeError> {
    Ok(!matches!(set.inspect(), ValueView::Nil) && set.contains(value)?)
}
fn remove(c: &Context, set: &RootedValue, value: &RootedValue) -> NativeResult {
    if matches!(set.inspect(), ValueView::Nil) {
        Ok(set.clone())
    } else {
        Ok(c.disj(set, value)?)
    }
}
#[derive(Clone, Copy)]
enum Algebra {
    Union,
    Intersection,
    Difference,
}
fn algebra(c: &mut Context, a: &[RootedValue], op: Algebra) -> NativeResult {
    for (i, value) in a.iter().enumerate() {
        set_count(value, i)?;
    }
    if a.is_empty() {
        return Ok(c.set(&[])?);
    }
    // Clojure's variadic bubble-max-key selects the last tied largest/smallest
    // set, retaining that set's metadata and persistent storage as the base.
    let start = if a.len() > 2 && !matches!(op, Algebra::Difference) {
        let mut start = 0;
        for i in 1..a.len() {
            let candidate = set_count(&a[i], i)?;
            let selected = set_count(&a[start], start)?;
            if match op {
                Algebra::Union => candidate >= selected,
                _ => candidate <= selected,
            } {
                start = i;
            }
        }
        start
    } else {
        0
    };
    let mut result = a[start].clone();
    for (i, value) in a.iter().enumerate() {
        if i == start {
            continue;
        }
        let (mut target, source) = match op {
            Algebra::Union if set_count(&result, 0)? < set_count(value, 1)? => {
                (value.clone(), result)
            }
            Algebra::Intersection if set_count(&result, 0)? > set_count(value, 1)? => {
                (value.clone(), result)
            }
            _ => (result, value.clone()),
        };
        match op {
            Algebra::Union => {
                let mut cursor = Cursor::new(c, &source, 0)?;
                while let Some(v) = cursor.next(c)? {
                    target = collection_core::conj(c, &target, &v)?;
                }
            }
            Algebra::Intersection => {
                let mut cursor = Cursor::new(c, &target, 0)?;
                while let Some(v) = cursor.next(c)? {
                    if !contains(&source, &v)? {
                        target = remove(c, &target, &v)?;
                    }
                }
            }
            Algebra::Difference => {
                let mut cursor = Cursor::new(c, &source, 0)?;
                while let Some(v) = cursor.next(c)? {
                    target = remove(c, &target, &v)?;
                }
            }
        }
        result = target;
    }
    Ok(result)
}
fn subset(c: &mut Context, a: &[RootedValue], reverse: bool) -> NativeResult {
    let (left, right) = if reverse {
        (&a[1], &a[0])
    } else {
        (&a[0], &a[1])
    };
    if set_count(left, 0)? > set_count(right, 1)? {
        return Ok(c.boolean(false));
    }
    let mut cursor = Cursor::new(c, left, 0)?;
    while let Some(v) = cursor.next(c)? {
        if !contains(right, &v)? {
            return Ok(c.boolean(false));
        }
    }
    Ok(c.boolean(true))
}
fn select(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    set_count(&a[1], 1)?;
    let mut result = a[1].clone();
    let mut cursor = Cursor::new(c, &a[1], 1)?;
    while let Some(v) = cursor.next(c)? {
        if !truthy(&c.invoke(&a[0], std::slice::from_ref(&v))?) {
            result = remove(c, &result, &v)?;
        }
    }
    Ok(result)
}
fn select_keys(c: &mut Context, value: &RootedValue, keys: &RootedValue) -> NativeResult {
    map(value, 0)?;
    collection_core::select_keys(c, value, keys)
}

fn rename_keys(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    map(&a[0], 0)?;
    map(&a[1], 1)?;
    if matches!(a[1].inspect(), ValueView::Nil) {
        return Ok(a[0].clone());
    }
    let pairs = a[1].entries()?;
    let mut result = a[0].clone();
    // Read values from the original map, so swaps and chains are simultaneous.
    for (old, _) in &pairs {
        c.checkpoint(PollPoint::Iteration)?;
        if !matches!(result.inspect(), ValueView::Nil) {
            result = c.dissoc(&result, old)?;
        }
    }
    for (old, new) in &pairs {
        c.checkpoint(PollPoint::Iteration)?;
        if !matches!(a[0].inspect(), ValueView::Nil)
            && let Some(value) = a[0].get(old)?
        {
            result = c.assoc(&result, new, &value)?;
        }
    }
    Ok(result)
}
fn project(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let mut result = c.set(&[])?;
    let mut rows = Cursor::new(c, &a[0], 0)?;
    while let Some(row) = rows.next(c)? {
        let key = select_keys(c, &row, &a[1])?;
        result = c.conj(&result, &key)?;
    }
    if let Some(meta) = a[0].metadata() {
        result = c.with_meta(&result, &meta)?;
    }
    Ok(result)
}
fn index(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    let mut result = c.map(&[])?;
    let mut rows = Cursor::new(c, &a[0], 0)?;
    while let Some(row) = rows.next(c)? {
        let key = select_keys(c, &row, &a[1])?;
        let group = match result.get(&key)? {
            Some(group) => group,
            None => c.set(&[])?,
        };
        let group = collection_core::conj(c, &group, &row)?;
        result = c.assoc(&result, &key, &group)?;
    }
    Ok(result)
}
fn relation(c: &mut Context, value: &RootedValue, argument: usize) -> NativeResult {
    let mut result = c.vector(&[])?;
    let mut rows = Cursor::new(c, value, argument)?;
    while let Some(row) = rows.next(c)? {
        map(&row, argument)?;
        result = c.conj(&result, &row)?;
    }
    Ok(result)
}
fn keys(c: &Context, value: &RootedValue) -> NativeResult {
    if matches!(value.inspect(), ValueView::Nil) {
        return Ok(c.set(&[])?);
    }
    Ok(c.set(
        &value
            .entries()?
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
    )?)
}
fn join(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    // Natural join only inspects the second relation once the first has a row.
    // An empty side must not force the rest of an infinite/effectful other side.
    if a.len() == 2
        && (Cursor::new(c, &a[0], 0)?.is_empty() || Cursor::new(c, &a[1], 1)?.is_empty())
    {
        return Ok(c.set(&[])?);
    }
    let left = relation(c, &a[0], 0)?;
    let right = relation(c, &a[1], 1)?;
    if let Some(mapping) = a.get(2) {
        map(mapping, 2)?;
    }
    let mut result = c.set(&[])?;
    if left.count() == Some(0) || right.count() == Some(0) {
        return Ok(result);
    }
    let swap = left.count() > right.count();
    let (small, large) = if swap {
        (&right, &left)
    } else {
        (&left, &right)
    };
    let (small_keys, large_keys, mapping) = if let Some(mapping) = a.get(2) {
        let mut oriented = c.map(&[])?;
        if !matches!(mapping.inspect(), ValueView::Nil) {
            for (left, right) in mapping.entries()? {
                c.checkpoint(PollPoint::Iteration)?;
                let (from, to) = if swap { (left, right) } else { (right, left) };
                oriented = c.assoc(&oriented, &from, &to)?;
            }
        }
        let from = keys(c, &oriented)?;
        let to = c.set(
            &oriented
                .entries()?
                .into_iter()
                .map(|(_, v)| v)
                .collect::<Vec<_>>(),
        )?;
        (to, from, Some(oriented))
    } else {
        let left_keys = keys(c, &left.nth(0)?.unwrap())?;
        let right_keys = keys(c, &right.nth(0)?.unwrap())?;
        let common = algebra(c, &[left_keys, right_keys], Algebra::Intersection)?;
        (common.clone(), common, None)
    };
    let indexed = index(c, &[small.clone(), small_keys])?;
    let mut rows = Cursor::new(c, large, 1)?;
    while let Some(row) = rows.next(c)? {
        let mut key = select_keys(c, &row, &large_keys)?;
        if let Some(mapping) = &mapping {
            key = rename_keys(c, &[key, mapping.clone()])?;
        }
        if let Some(matches) = indexed.get(&key)? {
            let mut matches = Cursor::new(c, &matches, 0)?;
            while let Some(found) = matches.next(c)? {
                let mut merged = found;
                if !matches!(row.inspect(), ValueView::Nil) {
                    if matches!(merged.inspect(), ValueView::Nil) {
                        merged = c.map(&[])?;
                    }
                    for (key, value) in row.entries()? {
                        c.checkpoint(PollPoint::Iteration)?;
                        merged = c.assoc(&merged, &key, &value)?;
                    }
                }
                result = c.conj(&result, &merged)?;
            }
        }
    }
    Ok(result)
}
pub fn set_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $arity:expr, $body:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.set",
                    name: $name,
                    arity: $arity,
                    effects: NativeEffects::Context,
                    errors: &[
                        NativeErrorClass::Type,
                        NativeErrorClass::Collection,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: $body,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("union", Arity::AtLeast(0), |c, a| algebra(
            c,
            a,
            Algebra::Union
        )),
        def!("intersection", Arity::AtLeast(1), |c, a| algebra(
            c,
            a,
            Algebra::Intersection
        )),
        def!("difference", Arity::AtLeast(1), |c, a| algebra(
            c,
            a,
            Algebra::Difference
        )),
        def!("subset?", Arity::Exact(2), |c, a| subset(c, a, false)),
        def!("superset?", Arity::Exact(2), |c, a| subset(c, a, true)),
        def!("select", Arity::Exact(2), select),
        def!("rename-keys", Arity::Exact(2), rename_keys),
        def!("project", Arity::Exact(2), project),
        def!("index", Arity::Exact(2), index),
        def!("join", Arity::Range { min: 2, max: 3 }, join),
    ];
    DEFINITIONS
}
