//! Unchunked core sequences. Each thunk captures only traced runtime values;
//! successful heads are memoized by Deferred and callbacks use Context::invoke.
use super::*;
use collection_core::{sequence, truthy};
use numbers::Operation as Number;

#[derive(Clone, Copy)]
pub(super) enum Producer {
    Map,
    MapIndexed,
    Filter,
    Remove,
    Keep,
    KeepIndexed,
    Take,
    Drop,
    TakeWhile,
    DropWhile,
    Range,
    RangeNext,
    Repeat,
    Repeatedly,
    Iterate,
    Cycle,
    Partition,
    PartitionPad,
    PartitionAll,
    Interpose,
    InterposeTail,
    Interleave,
    FlattenOne,
    TakeNth,
    Empty,
    FailedType,
}
fn poll(c: &mut Context) -> Result<(), NativeError> {
    c.poll_execution(PollPoint::Iteration)
        .map_err(InvokeError::Dispatch)?;
    Ok(())
}
fn numeric(c: &mut Context, a: &[RootedValue], op: Number) -> NativeResult {
    numbers::call(c, a, op)
}
fn positive(c: &mut Context, n: &RootedValue) -> Result<bool, NativeError> {
    Ok(truthy(&numeric(
        c,
        std::slice::from_ref(n),
        Number::Positive,
    )?))
}
fn decrement(c: &mut Context, n: &RootedValue) -> NativeResult {
    numeric(c, std::slice::from_ref(n), Number::Dec)
}
fn empty(v: &RootedValue) -> bool {
    matches!(v.inspect(), ValueView::Nil)
}
impl Context {
    pub(super) fn lazy_core(&self, op: Producer, captures: &[RootedValue]) -> NativeResult {
        let body = self.higher_order(sequence_core::Combinator::Lazy(op), captures)?;
        self.deferred_retry(&body, false, true)
    }
}
fn node(c: &mut Context, head: &RootedValue, op: Producer, args: &[RootedValue]) -> NativeResult {
    let tail = c.lazy_core(op, args)?;
    c.lazy_cons(head, &tail)
}
fn head(seq: &RootedValue) -> Result<RootedValue, NativeError> {
    Ok(seq.nth(0)?.expect("established nonempty sequence"))
}
pub(super) fn invoke_producer(
    c: &mut Context,
    callee: &RootedValue,
    captures: &[RootedValue],
    op: Producer,
) -> NativeResult {
    let result = produce(c, callee, captures, op);
    if result.is_err() && Rc::ptr_eq(&c.owner, &callee.owner) {
        // The JVM host's once-only lazy closures clear sequence captures on
        // invocation. Preserve the observable retry category without keeping
        // failed callbacks (or replaying their completed effects).
        use Producer::*;
        let next = match op {
            Drop | DropWhile | Interpose | InterposeTail => Some(FailedType),
            Take if !matches!(
                captures[0].inspect(),
                ValueView::Integer(_) | ValueView::Double(_) | ValueView::Ratio { .. }
            ) =>
            {
                None
            }
            Map | MapIndexed | Filter | Remove | Keep | KeepIndexed | Take | TakeWhile
            | Partition | PartitionPad | PartitionAll | Interleave | TakeNth => Some(Empty),
            _ => None,
        };
        if let Some(next) = next {
            let mut store = c.owner.store.borrow_mut();
            let Value::Heap(id) = store.unannotated(callee.value()) else {
                unreachable!()
            };
            let HeapValue::HigherOrder(function) = store.heap.get_mut(id.0) else {
                unreachable!()
            };
            function.operation = sequence_core::Combinator::Lazy(next);
            function.captures = Box::new([]);
        }
    }
    result
}
fn produce(c: &mut Context, callee: &RootedValue, a: &[RootedValue], op: Producer) -> NativeResult {
    use Producer::*;
    poll(c)?;
    match op {
        Empty => Ok(c.nil()),
        FailedType => Err(NativeError::Type {
            argument: 0,
            expected: "valid lazy producer state",
        }),
        Map | MapIndexed => {
            let indexed = matches!(op, MapIndexed);
            let offset = if indexed { 2 } else { 1 };
            let mut inputs = Vec::with_capacity(a.len() - offset);
            // Fixed host map arities establish all inputs before checking emptiness.
            for source in &a[offset..] {
                let s = sequence(c, source, offset)?;
                if !indexed && a.len() > 4 && empty(&s) {
                    return Ok(c.nil());
                }
                inputs.push(s);
            }
            if inputs.iter().any(empty) {
                return Ok(c.nil());
            }
            let mut args = Vec::with_capacity(inputs.len() + usize::from(indexed));
            let mut rest = vec![a[0].clone()];
            if indexed {
                args.push(a[1].clone());
                rest.push(numeric(c, &a[1..2], Number::Inc)?);
            }
            for s in inputs {
                args.push(head(&s)?);
                rest.push(c.sequence_tail(&s)?);
            }
            let value = c.invoke(&a[0], &args)?;
            node(c, &value, op, &rest)
        }
        Filter | Remove | Keep | KeepIndexed => {
            let indexed = matches!(op, KeepIndexed);
            let mut index = if indexed { a[1].clone() } else { c.integer(0) };
            let mut source = a.last().unwrap().clone();
            loop {
                poll(c)?;
                let s = sequence(c, &source, a.len() - 1)?;
                if empty(&s) {
                    return Ok(c.nil());
                }
                let value = head(&s)?;
                let args = if indexed {
                    vec![index.clone(), value.clone()]
                } else {
                    vec![value.clone()]
                };
                let selected = c.invoke(&a[0], &args)?;
                source = c.sequence_tail(&s)?;
                if indexed {
                    index = numeric(c, &[index], Number::Inc)?;
                }
                let keep = match op {
                    Filter => truthy(&selected),
                    Remove => !truthy(&selected),
                    _ => !empty(&selected),
                };
                if keep {
                    let result = if matches!(op, Keep | KeepIndexed) {
                        selected
                    } else {
                        value
                    };
                    let mut rest = vec![a[0].clone()];
                    if indexed {
                        rest.push(index);
                    }
                    rest.push(source);
                    return node(c, &result, op, &rest);
                }
            }
        }
        Take => {
            if !positive(c, &a[0])? {
                return Ok(c.nil());
            }
            let s = sequence(c, &a[1], 1)?;
            if empty(&s) {
                return Ok(c.nil());
            }
            let n = decrement(c, &a[0])?;
            let tail = c.sequence_tail(&s)?;
            node(c, &head(&s)?, Take, &[n, tail])
        }
        Drop => {
            let mut n = a[0].clone();
            let mut s = sequence(c, &a[1], 1)?;
            while positive(c, &n)? && !empty(&s) {
                poll(c)?;
                n = decrement(c, &n)?;
                let tail = c.sequence_tail(&s)?;
                s = sequence(c, &tail, 1)?;
            }
            Ok(s)
        }
        TakeWhile | DropWhile => {
            let mut source = a[1].clone();
            loop {
                poll(c)?;
                let s = sequence(c, &source, 1)?;
                if empty(&s) {
                    return Ok(c.nil());
                }
                let value = head(&s)?;
                let selected = c.invoke(&a[0], std::slice::from_ref(&value))?;
                if matches!(op, TakeWhile) {
                    if !truthy(&selected) {
                        return Ok(c.nil());
                    }
                    return node(c, &value, op, &[a[0].clone(), c.sequence_tail(&s)?]);
                }
                if !truthy(&selected) {
                    return Ok(s);
                }
                source = c.sequence_tail(&s)?;
            }
        }
        RangeNext => {
            // A bounded integral range may cross its exclusive bound without
            // ever producing an out-of-profile integer.
            if let (ValueView::Integer(start), ValueView::Integer(end), ValueView::Integer(step)) =
                (a[0].inspect(), a[1].inspect(), a[2].inspect())
            {
                let next = i128::from(start) + i128::from(step);
                if step > 0 && next >= i128::from(end) || step < 0 && next <= i128::from(end) {
                    return Ok(c.nil());
                }
            }
            let next = numeric(c, &[a[0].clone(), a[2].clone()], Number::Add)?;
            produce(c, callee, &[next, a[1].clone(), a[2].clone()], Range)
        }
        Range => {
            if !empty(&a[1]) {
                let positive_step = positive(c, &a[2])?;
                let negative_step = truthy(&numeric(c, &a[2..3], Number::Negative)?);
                let cmp = if positive_step {
                    Number::GreaterEqual
                } else if negative_step {
                    Number::LessEqual
                } else {
                    Number::Equal
                };
                if truthy(&numeric(c, &a[..2], cmp)?) {
                    return Ok(c.nil());
                }
            }
            node(c, &a[0], RangeNext, a)
        }
        Repeat => node(c, &a[0], Repeat, a),
        Repeatedly => {
            let value = c.invoke(&a[0], &[])?;
            node(c, &value, Repeatedly, a)
        }
        Iterate => {
            let value = c.invoke(&a[0], &a[1..])?;
            node(c, &value, Iterate, &[a[0].clone(), value.clone()])
        }
        Cycle => {
            let mut s = sequence(c, &a[1], 0)?;
            if empty(&s) {
                s = sequence(c, &a[0], 0)?;
            }
            if empty(&s) {
                return Ok(c.nil());
            }
            node(c, &head(&s)?, Cycle, &[a[0].clone(), c.sequence_tail(&s)?])
        }
        Partition | PartitionPad | PartitionAll => {
            let s = sequence(c, &a[3], 3)?;
            if empty(&s) {
                return Ok(c.nil());
            }
            let group = c.lazy_core(Take, &[a[0].clone(), s.clone()])?;
            let values = c.sequence_elements(&group)?;
            if matches!(op, PartitionAll) {
                let rest = call(c, &[s, a[1].clone()], Operation::Nthrest)?;
                return node(
                    c,
                    &group,
                    op,
                    &[a[0].clone(), a[1].clone(), a[2].clone(), rest],
                );
            }
            let count = c.integer(values.len() as i64);
            if c.value_eq(&count, &a[0])? {
                let rest = call(c, &[s, a[1].clone()], Operation::Nthrest)?;
                return node(
                    c,
                    &group,
                    op,
                    &[a[0].clone(), a[1].clone(), a[2].clone(), rest],
                );
            }
            if matches!(op, PartitionPad) {
                let joined = c.lazy_concat(&[group, a[2].clone()])?;
                let padded = c.lazy_core(Take, &[a[0].clone(), joined])?;
                return c.lazy_cons(&padded, &c.nil());
            }
            Ok(c.nil())
        }
        Interpose | InterposeTail => {
            let s = sequence(c, &a[1], 1)?;
            if empty(&s) {
                return Ok(c.nil());
            }
            let rest = c.sequence_tail(&s)?;
            let value = node(c, &head(&s)?, InterposeTail, &[a[0].clone(), rest])?;
            if matches!(op, InterposeTail) {
                c.lazy_cons(&a[0], &value)
            } else {
                Ok(value)
            }
        }
        Interleave => {
            if a.is_empty() {
                return Ok(c.nil());
            }
            let mut seqs = Vec::with_capacity(a.len());
            for source in a {
                let s = sequence(c, source, seqs.len())?;
                if a.len() != 2 && empty(&s) {
                    return Ok(c.nil());
                }
                seqs.push(s);
            }
            if seqs.iter().any(empty) {
                return Ok(c.nil());
            }
            let rest = seqs
                .iter()
                .map(|s| c.sequence_tail(s))
                .collect::<Result<Vec<_>, _>>()?;
            let mut result = c.lazy_core(Interleave, &rest)?;
            for s in seqs.iter().rev() {
                result = c.lazy_cons(&head(s)?, &result)?;
            }
            Ok(result)
        }
        FlattenOne => {
            let mut outer = a[0].clone();
            let mut inner = a[1].clone();
            loop {
                poll(c)?;
                let s = match sequence(c, &inner, 0) {
                    Ok(s) => s,
                    Err(error) => {
                        if Rc::ptr_eq(&c.owner, &callee.owner) {
                            let mut store = c.owner.store.borrow_mut();
                            let Value::Heap(id) = store.unannotated(callee.value()) else {
                                unreachable!()
                            };
                            let HeapValue::HigherOrder(function) = store.heap.get_mut(id.0) else {
                                unreachable!()
                            };
                            function.captures[0] = outer.value();
                            function.captures[1] = Value::Nil;
                        }
                        return Err(error);
                    }
                };
                if !empty(&s) {
                    return node(c, &head(&s)?, FlattenOne, &[outer, c.sequence_tail(&s)?]);
                }
                let more = sequence(c, &outer, 0)?;
                if empty(&more) {
                    return Ok(c.nil());
                }
                inner = head(&more)?;
                outer = c.sequence_tail(&more)?;
            }
        }
        TakeNth => {
            let s = sequence(c, &a[1], 1)?;
            if empty(&s) {
                return Ok(c.nil());
            }
            let rest = c.lazy_core(Drop, &[a[0].clone(), s.clone()])?;
            node(c, &head(&s)?, TakeNth, &[a[0].clone(), rest])
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Lazy(Producer),
    Mapcat,
    Range,
    Repeat,
    Repeatedly,
    Iterate,
    Cycle,
    Partition,
    PartitionAll,
    Doall,
    Dorun,
    Second,
    Ffirst,
    Nfirst,
    Fnext,
    Nnext,
    Last,
    Butlast,
    Nthnext,
    Nthrest,
    SplitAt,
    SplitWith,
}
fn call(c: &mut Context, a: &[RootedValue], op: Operation) -> NativeResult {
    use Operation::*;
    match op {
        Lazy(
            producer @ (Producer::Map
            | Producer::Filter
            | Producer::Remove
            | Producer::Take
            | Producer::Drop
            | Producer::TakeWhile),
        ) if a.len() == 1 => {
            use sequence_core::{Combinator, Transform};
            let transform = match producer {
                Producer::Map => Transform::Map,
                Producer::Filter => Transform::Filter,
                Producer::Remove => Transform::Remove,
                Producer::Take => Transform::Take,
                Producer::Drop => Transform::Drop,
                Producer::TakeWhile => Transform::TakeWhile,
                _ => unreachable!(),
            };
            c.higher_order(Combinator::Transform(transform), a)
        }
        Lazy(Producer::MapIndexed | Producer::KeepIndexed) => {
            let Lazy(producer) = op else { unreachable!() };
            c.lazy_core(producer, &[a[0].clone(), c.integer(0), a[1].clone()])
        }
        Lazy(producer) => c.lazy_core(producer, a),
        Mapcat if a.len() == 1 => {
            use sequence_core::{Combinator, Transform};
            let mapped = c.higher_order(Combinator::Transform(Transform::Map), a)?;
            let cat = c.native_function(
                sequence_definitions()
                    .iter()
                    .find(|d| d.metadata.name == "cat")
                    .unwrap(),
            );
            c.higher_order(Combinator::Comp, &[mapped, cat])
        }
        Mapcat => {
            let mapped = c.lazy_core(Producer::Map, a)?;
            let definition = sequence_definitions()
                .iter()
                .find(|d| d.metadata.name == "concat")
                .unwrap();
            let concat = c.native_function(definition);
            Ok(c.invoke_sequence(&concat, &mapped)?)
        }
        Range => {
            let start = if a.len() >= 2 {
                a[0].clone()
            } else {
                c.integer(0)
            };
            let end = match a.len() {
                0 => c.nil(),
                1 => a[0].clone(),
                _ => a[1].clone(),
            };
            let step = a.get(2).cloned().unwrap_or_else(|| c.integer(1));
            // Range validates numeric arguments at construction, without building a prefix.
            numeric(c, std::slice::from_ref(&start), Number::Positive)?;
            numeric(c, std::slice::from_ref(&step), Number::Positive)?;
            if !a.is_empty() {
                numeric(c, std::slice::from_ref(&end), Number::Positive)?;
            }
            if a.len() == 1 && matches!(end.inspect(), ValueView::Double(n) if n.is_nan()) {
                return c.lazy_core(Producer::Take, &[c.integer(0), c.nil()]);
            }
            c.lazy_core(Producer::Range, &[start, end, step])
        }
        Repeat | Repeatedly => {
            let producer = if matches!(op, Repeat) {
                Producer::Repeat
            } else {
                Producer::Repeatedly
            };
            let source = c.lazy_core(producer, &a[a.len() - 1..])?;
            if a.len() == 1 {
                Ok(source)
            } else {
                let count = if matches!(op, Repeat) {
                    if matches!(a[0].inspect(), ValueView::Double(n) if n.is_nan()) {
                        c.integer(0)
                    } else {
                        numeric(c, &a[..1], Number::Long)?
                    }
                } else {
                    a[0].clone()
                };
                c.lazy_core(Producer::Take, &[count, source])
            }
        }
        Iterate => node(c, &a[1], Producer::Iterate, a),
        Cycle => {
            let source = sequence(c, &a[0], 0)?;
            c.lazy_core(Producer::Cycle, &[source.clone(), source])
        }
        PartitionAll if a.len() == 1 => {
            // The ^long transducer entry truncates numeric counts immediately;
            // collection-taking arities deliberately keep their own semantics.
            let invalid = || NativeError::Type {
                argument: 0,
                expected: "number in signed 64-bit partition range",
            };
            match a[0].inspect() {
                ValueView::Integer(_) | ValueView::Ratio { .. } => {}
                ValueView::Double(n)
                    if n.is_nan()
                        || (-9_223_372_036_854_775_808.0..=9_223_372_036_854_775_808.0)
                            .contains(&n) => {}
                _ => return Err(invalid()),
            }
            // At the rounded double representation of Long/MAX_VALUE the
            // pinned host accepts the bound and saturates its primitive cast.
            let n = if matches!(a[0].inspect(), ValueView::Double(n) if n == i64::MAX as f64) {
                c.integer(i64::MAX)
            } else {
                numeric(c, a, Number::Long)?
            };
            c.higher_order(
                sequence_core::Combinator::Transform(sequence_core::Transform::PartitionAll),
                &[n],
            )
        }
        Partition | PartitionAll => {
            let n = a[0].clone();
            let step = if a.len() == 2 {
                n.clone()
            } else {
                a[1].clone()
            };
            let pad = if a.len() == 4 { a[2].clone() } else { c.nil() };
            let producer = if matches!(op, PartitionAll) {
                Producer::PartitionAll
            } else if a.len() == 4 {
                Producer::PartitionPad
            } else {
                Producer::Partition
            };
            c.lazy_core(producer, &[n, step, pad, a.last().unwrap().clone()])
        }
        Doall | Dorun => {
            let original = a.last().unwrap();
            let mut source = original.clone();
            let mut count = a.first().filter(|_| a.len() == 2).cloned();
            loop {
                poll(c)?;
                // dorun tests seq before its bounded count, and next establishes
                // the following link after each consumed element.
                let s = sequence(c, &source, a.len() - 1)?;
                if empty(&s) {
                    break;
                }
                if let Some(n) = &count {
                    if !positive(c, n)? {
                        break;
                    }
                    count = Some(decrement(c, n)?);
                }
                source = c.sequence_tail(&s)?;
            }
            if matches!(op, Doall) {
                Ok(original.clone())
            } else {
                Ok(c.nil())
            }
        }
        Last | Butlast => {
            let mut cursor = collection_core::Cursor::new(c, &a[0], 0)?;
            let mut previous = cursor.next(c)?;
            let mut values = Vec::new();
            while let Some(next) = cursor.next(c)? {
                if matches!(op, Butlast) {
                    c.charge_allocation(1)?;
                    values.push(previous.take().unwrap());
                }
                previous = Some(next);
            }
            if matches!(op, Last) {
                Ok(previous.unwrap_or_else(|| c.nil()))
            } else {
                Ok(c.eager_sequence(&values, true)?)
            }
        }
        Nthnext | Nthrest => {
            let mut source = if matches!(op, Nthnext) {
                sequence(c, &a[0], 0)?
            } else {
                a[0].clone()
            };
            let mut n = a[1].clone();
            loop {
                if matches!(op, Nthnext) && empty(&source) {
                    return Ok(source);
                }
                if !positive(c, &n)? {
                    return Ok(source);
                }
                poll(c)?;
                let s = sequence(c, &source, 0)?;
                let tail = c.sequence_tail(&s)?;
                source = sequence(c, &tail, 0)?;
                if empty(&source) {
                    return if matches!(op, Nthrest) {
                        Ok(c.list(&[])?)
                    } else {
                        Ok(source)
                    };
                }
                n = decrement(c, &n)?;
            }
        }
        SplitAt | SplitWith => {
            let (left, right) = if matches!(op, SplitAt) {
                (Producer::Take, Producer::Drop)
            } else {
                (Producer::TakeWhile, Producer::DropWhile)
            };
            Ok(c.vector(&[c.lazy_core(left, a)?, c.lazy_core(right, a)?])?)
        }
        Second | Ffirst | Nfirst | Fnext | Nnext => {
            let s = sequence(c, &a[0], 0)?;
            let source = if matches!(op, Ffirst | Nfirst) {
                if empty(&s) { c.nil() } else { head(&s)? }
            } else {
                c.sequence_tail(&s)?
            };
            let s = sequence(c, &source, 0)?;
            if matches!(op, Nfirst | Nnext) {
                let tail = c.sequence_tail(&s)?;
                sequence(c, &tail, 0)
            } else if empty(&s) {
                Ok(c.nil())
            } else {
                head(&s)
            }
        }
    }
}
macro_rules! def {
    ($name:literal, $arity:expr, $op:expr) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::Context,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Type,
                    NativeErrorClass::Collection,
                    NativeErrorClass::Invocation,
                    NativeErrorClass::Arithmetic,
                ],
            },
            callback: |c, a| call(c, a, $op),
        }
    };
}
pub fn lazy_definitions() -> &'static [NativeDefinition] {
    use Arity::{AtLeast, Exact, Range as ArityRange};
    use Operation::*;
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("map", AtLeast(1), Lazy(Producer::Map)),
        def!("map-indexed", Exact(2), Lazy(Producer::MapIndexed)),
        def!("mapcat", AtLeast(1), Mapcat),
        def!(
            "filter",
            ArityRange { min: 1, max: 2 },
            Lazy(Producer::Filter)
        ),
        def!(
            "remove",
            ArityRange { min: 1, max: 2 },
            Lazy(Producer::Remove)
        ),
        def!("keep", Exact(2), Lazy(Producer::Keep)),
        def!("keep-indexed", Exact(2), Lazy(Producer::KeepIndexed)),
        def!("take", ArityRange { min: 1, max: 2 }, Lazy(Producer::Take)),
        def!("drop", ArityRange { min: 1, max: 2 }, Lazy(Producer::Drop)),
        def!(
            "take-while",
            ArityRange { min: 1, max: 2 },
            Lazy(Producer::TakeWhile)
        ),
        def!("drop-while", Exact(2), Lazy(Producer::DropWhile)),
        def!("range", ArityRange { min: 0, max: 3 }, Range),
        def!("repeat", ArityRange { min: 1, max: 2 }, Repeat),
        def!("repeatedly", ArityRange { min: 1, max: 2 }, Repeatedly),
        def!("iterate", Exact(2), Iterate),
        def!("cycle", Exact(1), Cycle),
        def!("partition", ArityRange { min: 2, max: 4 }, Partition),
        def!("partition-all", ArityRange { min: 1, max: 3 }, PartitionAll),
        def!("interpose", Exact(2), Lazy(Producer::Interpose)),
        def!("interleave", AtLeast(0), Lazy(Producer::Interleave)),
        def!("take-nth", Exact(2), Lazy(Producer::TakeNth)),
        def!("doall", ArityRange { min: 1, max: 2 }, Doall),
        def!("dorun", ArityRange { min: 1, max: 2 }, Dorun),
        def!("second", Exact(1), Second),
        def!("ffirst", Exact(1), Ffirst),
        def!("nfirst", Exact(1), Nfirst),
        def!("fnext", Exact(1), Fnext),
        def!("nnext", Exact(1), Nnext),
        def!("last", Exact(1), Last),
        def!("butlast", Exact(1), Butlast),
        def!("nthnext", Exact(2), Nthnext),
        def!("nthrest", Exact(2), Nthrest),
        def!("split-at", Exact(2), SplitAt),
        def!("split-with", Exact(2), SplitWith),
    ];
    DEFINITIONS
}
