//! Finite traversal and eager higher-order functions. Callbacks use the common
//! evaluator; every retained edge is traced and no store borrow crosses a call.
use super::*;
use collection_core::{Cursor, sequence, truthy};

#[derive(Clone, Copy)]
pub(super) enum Transform {
    Map,
    Filter,
    Remove,
    Take,
    Drop,
    TakeWhile,
    PartitionAll,
}

#[derive(Clone, Copy)]
pub(super) enum Combinator {
    Comp,
    Partial,
    Complement,
    Completing,
    Cat,
    PreservingReduced,
    Transform(Transform),
    Reducing(Transform),
    Constantly,
    Juxt,
    LazyConcat,
    LazyCat,
    LazyKeys,
    LazyVals,
    Lazy(lazy_core::Producer),
}
pub(super) struct HigherOrder {
    pub(super) operation: Combinator,
    pub(super) captures: Box<[Value]>,
    pub(super) tail: Option<Value>,
}
impl Context {
    pub(super) fn higher_order(
        &self,
        operation: Combinator,
        captures: &[RootedValue],
    ) -> NativeResult {
        self.higher_order_tail(operation, captures, None)
    }
    fn higher_order_tail(
        &self,
        operation: Combinator,
        captures: &[RootedValue],
        tail: Option<&RootedValue>,
    ) -> NativeResult {
        self.charge_allocation(captures.len().saturating_add(1) as u64)?;
        let raw = Value::Heap(HeapId(self.owner.store.borrow_mut().heap.insert(
            HeapValue::HigherOrder(HigherOrder {
                operation,
                captures: captures.iter().map(RootedValue::value).collect(),
                tail: tail.map(RootedValue::value),
            }),
        )));
        Ok(self.owner.checked_value(raw)?)
    }
    /// Wrap a value as an early-reduction result. Markers nest and use identity
    /// equality, matching Reduced objects rather than their contained values.
    pub fn reduced(&self, value: &RootedValue) -> NativeResult {
        self.charge_allocation(1)?;
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        let raw = Value::Heap(HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Reduced(value.value())),
        ));
        Ok(self.owner.checked_value(raw)?)
    }
}
impl RootedValue {
    /// Inspect one marker layer, retaining the original owner after context drop.
    pub fn reduced_value(&self) -> Option<RootedValue> {
        let store = self.owner.store.borrow();
        let HeapValue::Reduced(value) = store.heap_value(self.value())? else {
            return None;
        };
        Some(self.owner.value(*value))
    }
}
fn poll(context: &mut Context) -> Result<(), NativeError> {
    context
        .poll_execution(PollPoint::Iteration)
        .map_err(|e| NativeError::from(InvokeError::Dispatch(e)))
}

pub(super) fn force_arguments(
    context: &mut Context,
    source: &RootedValue,
    output: &mut Vec<RootedValue>,
) -> Result<(), InvokeError> {
    let force = (|| -> Result<(), NativeError> {
        let mut cursor = Cursor::new(context, source, 0)?;
        loop {
            poll(context)?;
            let Some(value) = cursor.next(context)? else {
                break;
            };
            output.push(value);
        }
        Ok(())
    })();
    force.map_err(|error| match error {
        NativeError::Invocation(error) => error.into_error(),
        error => InvokeError::Dispatch(EvalErrorKind::Native {
            name: Box::new(Name::new(Some("clojure.core"), "apply")),
            error,
        }),
    })
}

pub(super) fn applied_prefix(
    context: &mut Context,
    source: &RootedValue,
    required: usize,
) -> Result<(), InvokeError> {
    let mut tail = source.clone();
    // RestFn boundedLength establishes one link beyond the required prefix.
    for _ in 0..required.saturating_add(1) {
        context
            .poll_execution(PollPoint::Iteration)
            .map_err(InvokeError::Dispatch)?;
        tail = sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
        if matches!(tail.inspect(), ValueView::Nil) {
            break;
        }
        tail = context.sequence_tail(&tail)?;
        tail = sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
    }
    Ok(())
}

pub(super) fn apply_constructor(
    context: &mut Context,
    definition: &'static NativeDefinition,
    source: &RootedValue,
) -> Option<NativeResult> {
    let threshold = match definition.metadata.name {
        "concat" => 2,
        "partial" | "mapv" => 4,
        "juxt" => 3,
        _ => return None,
    };
    if !sequence_definitions()
        .iter()
        .any(|candidate| std::ptr::eq(candidate, definition))
    {
        return None;
    }
    let count = match source.count() {
        Some(n) => n,
        None => match (|| -> Result<usize, NativeError> {
            let mut cursor = Cursor::new(context, source, 0)?;
            let mut count = 0;
            while count <= threshold && cursor.next(context)?.is_some() {
                count += 1;
            }
            Ok(count)
        })() {
            Ok(n) => n,
            Err(e) => return Some(Err(e)),
        },
    };
    if count <= threshold {
        return None;
    }
    let mapv = definition.metadata.name == "mapv";
    let operation = if definition.metadata.name == "juxt" {
        Combinator::Juxt
    } else {
        Combinator::Partial
    };
    let prefix = if matches!(operation, Combinator::Partial) {
        4
    } else {
        3
    };
    if context.active_calls >= context.accounting.depths().0 {
        return Some(Err(InvokeError::Dispatch(
            context
                .accounting
                .kind(context.accounting.fail(ResourceError::CallDepthExceeded)),
        )
        .into()));
    }
    context.active_calls += 1;
    let result = (|| {
        context
            .poll_execution(PollPoint::Function)
            .map_err(InvokeError::Dispatch)?;
        if definition.metadata.name == "concat" {
            applied_prefix(context, source, 2)?;
            let first = sequence(context, source, 0)?;
            let second = sequence(context, &context.sequence_tail(&first)?, 0)?;
            let pair = context.lazy_concat(&[first.nth(0)?.unwrap(), second.nth(0)?.unwrap()])?;
            return context.lazy_core(
                lazy_core::Producer::FlattenOne,
                &[context.sequence_tail(&second)?, pair],
            );
        }
        applied_prefix(context, source, prefix)?;
        let mut tail = source.clone();
        let mut captures = Vec::with_capacity(prefix);
        for _ in 0..prefix {
            poll(context)?;
            tail = sequence(context, &tail, 0)?;
            captures.push(tail.nth(0)?.expect("constructor prefix"));
            tail = context.sequence_tail(&tail)?;
        }
        if mapv {
            for input in &captures[1..] {
                if Cursor::new(context, input, 1)?.is_empty() {
                    return Ok(context.vector(&[])?);
                }
            }
            let mut remaining = Cursor::new(context, &tail, 4)?;
            loop {
                poll(context)?;
                let Some(input) = remaining.next(context)? else {
                    break;
                };
                if Cursor::new(context, &input, captures.len())?.is_empty() {
                    return Ok(context.vector(&[])?);
                }
                captures.push(input);
            }
            call(context, &captures, Operation::Mapv)
        } else {
            context.higher_order_tail(operation, &captures, Some(&tail))
        }
    })();
    context.active_calls -= 1;
    Some(result)
}

pub(super) fn invoke_higher_order(
    context: &mut Context,
    callee: &RootedValue,
    args: &[RootedValue],
    applied: Option<&RootedValue>,
) -> Option<Result<RootedValue, InvokeError>> {
    let (operation, captures, tail) = {
        let store = callee.owner.store.borrow();
        let HeapValue::HigherOrder(function) = store.heap_value(callee.value())? else {
            return None;
        };
        (
            function.operation,
            function
                .captures
                .iter()
                .map(|v| callee.owner.value(*v))
                .collect::<Vec<_>>(),
            function.tail.map(|value| callee.owner.value(value)),
        )
    };
    let result = (|| {
        if context.active_calls >= context.accounting.depths().0 {
            return Err(InvokeError::Dispatch(
                context
                    .accounting
                    .kind(context.accounting.fail(ResourceError::CallDepthExceeded)),
            ));
        }
        context
            .poll_execution(PollPoint::Function)
            .map_err(InvokeError::Dispatch)?;
        // Clojure's combinators have fixed entry arities before their rest
        // entry. Demand that prefix, but preserve the remainder for callbacks.
        if let Some(source) = applied
            && !matches!(
                operation,
                Combinator::Completing
                    | Combinator::Cat
                    | Combinator::PreservingReduced
                    | Combinator::Transform(_)
                    | Combinator::Reducing(_)
            )
        {
            let prefix = match operation {
                Combinator::Constantly => 0,
                Combinator::Complement => 2,
                Combinator::Partial if captures.len() > 4 || tail.is_some() => 0,
                _ => 3,
            };
            applied_prefix(context, source, prefix)?;
            let mut tail = source.clone();
            for _ in 0..prefix.min(source.count().unwrap_or(prefix)) {
                tail = sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
                if matches!(tail.inspect(), ValueView::Nil) {
                    break;
                }
                context
                    .poll_execution(PollPoint::Iteration)
                    .map_err(InvokeError::Dispatch)?;
                tail.nth(0)
                    .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Collection(e)))?;
                tail = context.sequence_tail(&tail)?;
            }
        }
        context.active_calls += 1;
        let result = (|| match operation {
            Combinator::Lazy(producer) => {
                lazy_core::invoke_producer(context, callee, &captures, producer)
                    .map_err(deferred::invoke_error)
            }
            Combinator::LazyConcat | Combinator::LazyCat => deferred::produce_concat(
                context,
                callee,
                &captures,
                matches!(operation, Combinator::LazyCat),
            )
            .map_err(deferred::invoke_error),
            Combinator::LazyKeys | Combinator::LazyVals => deferred::produce_projection(
                context,
                &captures[0],
                usize::from(matches!(operation, Combinator::LazyVals)),
            )
            .map_err(deferred::invoke_error),
            Combinator::Constantly => Ok(captures[0].clone()),
            Combinator::Transform(transform) | Combinator::Reducing(transform) => {
                invoke_transform(context, operation, transform, &captures, args, applied)
            }
            Combinator::Completing | Combinator::Cat | Combinator::PreservingReduced => {
                // Like the fixed-arity reference adapter, applied calls count
                // the whole argument sequence, including on arity failure.
                // The shared traversal polls and charges each retained root.
                let mut values = Vec::new();
                let arguments = if let Some(source) = applied {
                    force_arguments(context, source, &mut values)?;
                    &values[..]
                } else {
                    args
                };
                if arguments.len() > 2 {
                    return Err(InvokeError::Dispatch(EvalErrorKind::Arity {
                        fixed: Box::new([0, 1, 2]),
                        variadic: None,
                        actual: arguments.len(),
                    }));
                }
                match operation {
                    Combinator::Cat if arguments.len() == 2 => {
                        reduce(context, &captures[1], Some(&arguments[0]), &arguments[1], 1)
                            .map_err(deferred::invoke_error)
                    }
                    Combinator::PreservingReduced => {
                        let result = context.invoke(&captures[0], arguments)?;
                        // Inner ordinary reduction removes one marker. Add just
                        // that layer so every enclosing cat still sees the stop.
                        if result.reduced_value().is_some() {
                            context.reduced(&result).map_err(deferred::invoke_error)
                        } else {
                            Ok(result)
                        }
                    }
                    _ => context.invoke(
                        &captures[usize::from(
                            matches!(operation, Combinator::Completing) && arguments.len() == 1,
                        )],
                        arguments,
                    ),
                }
            }
            Combinator::Complement => {
                let value = match applied {
                    Some(source) => context.invoke_sequence(&captures[0], source)?,
                    None => context.invoke(&captures[0], args)?,
                };
                Ok(context.boolean(!truthy(&value)))
            }
            Combinator::Partial => {
                if applied.is_some() || tail.is_some() {
                    let source = applied
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| context.eager_sequence(args, true))?;
                    let mut arguments = match &tail {
                        Some(tail) => context.concat_sequence(tail, &source)?,
                        None => source,
                    };
                    for capture in captures[1..].iter().rev() {
                        arguments = context
                            .cons_value(capture, &arguments)
                            .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Collection(e)))?;
                    }
                    return context.invoke_sequence(&captures[0], &arguments);
                }
                let mut arguments = Vec::with_capacity(captures.len() - 1 + args.len());
                arguments.extend_from_slice(&captures[1..]);
                arguments.extend_from_slice(args);
                context.invoke(&captures[0], &arguments)
            }
            Combinator::Comp => {
                let mut functions = captures.iter().rev();
                let first = functions.next().unwrap();
                let mut value = match applied {
                    Some(source) => context.invoke_sequence(first, source)?,
                    None => context.invoke(first, args)?,
                };
                for function in functions {
                    context
                        .poll_execution(PollPoint::Iteration)
                        .map_err(InvokeError::Dispatch)?;
                    value = context.invoke(function, &[value])?;
                }
                Ok(value)
            }
            Combinator::Juxt => {
                let mut values = Vec::with_capacity(captures.len());
                for function in &captures {
                    context
                        .poll_execution(PollPoint::Iteration)
                        .map_err(InvokeError::Dispatch)?;
                    values.push(match applied {
                        Some(source) => context.invoke_sequence(function, source)?,
                        None => context.invoke(function, args)?,
                    });
                }
                if let Some(tail) = &tail {
                    let mut functions = Cursor::new(context, tail, 0).map_err(|e| {
                        InvokeError::Dispatch(EvalErrorKind::Native {
                            name: Box::new(Name::new(Some("clojure.core"), "juxt")),
                            error: e,
                        })
                    })?;
                    loop {
                        context
                            .poll_execution(PollPoint::Iteration)
                            .map_err(InvokeError::Dispatch)?;
                        let Some(function) = functions.next(context).map_err(|e| {
                            InvokeError::Dispatch(EvalErrorKind::Native {
                                name: Box::new(Name::new(Some("clojure.core"), "juxt")),
                                error: e,
                            })
                        })?
                        else {
                            break;
                        };
                        values.push(match applied {
                            Some(source) => context.invoke_sequence(&function, source)?,
                            None => context.invoke(&function, args)?,
                        });
                    }
                }
                context
                    .vector(&values)
                    .map_err(|e| InvokeError::Dispatch(EvalErrorKind::Collection(e)))
            }
        })();
        context.active_calls -= 1;
        result
    })();
    Some(result)
}

fn invoke_transform(
    context: &mut Context,
    operation: Combinator,
    transform: Transform,
    captures: &[RootedValue],
    args: &[RootedValue],
    applied: Option<&RootedValue>,
) -> Result<RootedValue, InvokeError> {
    if matches!(operation, Combinator::Reducing(Transform::Map))
        && let Some(source) = applied
    {
        // The map reducer has a variadic step. Demand the ordinary RestFn
        // prefix, then preserve its input tail for the mapping function's apply.
        // A function that ignores its arguments can accept an unbounded tail.
        applied_prefix(context, source, 2)?;
        let source = sequence(context, source, 0).map_err(deferred::invoke_error)?;
        if matches!(source.inspect(), ValueView::Nil) {
            return context.invoke(&captures[1], &[]);
        }
        let accumulator = source.nth(0)?.expect("nonempty applied reducer arguments");
        let tail = context.sequence_tail(&source)?;
        let inputs = sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
        if matches!(inputs.inspect(), ValueView::Nil) {
            return context.invoke(&captures[1], &[accumulator]);
        }
        let mapped = context.invoke_sequence(&captures[0], &inputs)?;
        return context.invoke(&captures[1], &[accumulator, mapped]);
    }

    let mut values = Vec::new();
    let arguments = if let Some(source) = applied {
        // Constructors and non-map reducers have only fixed arities.
        force_arguments(context, source, &mut values)?;
        &values[..]
    } else {
        args
    };
    if matches!(operation, Combinator::Transform(_)) {
        if arguments.len() != 1 {
            return Err(InvokeError::Dispatch(EvalErrorKind::Arity {
                fixed: Box::new([1]),
                variadic: None,
                actual: arguments.len(),
            }));
        }
        if matches!(transform, Transform::PartitionAll) {
            // The constructor coerces to long; the reference's buffer capacity
            // narrows to int and rejects negatives only when applied to rf.
            let n = numbers::call(context, &captures[..1], numbers::Operation::Int)
                .map_err(deferred::invoke_error)?;
            if matches!(n.inspect(), ValueView::Integer(n) if n < 0) {
                return Err(deferred::invoke_error(NativeError::Type {
                    argument: 0,
                    expected: "nonnegative partition capacity",
                }));
            }
            // Grow the persistent buffer only with real inputs, never reserve n.
            let buffer = context
                .volatile(&context.vector(&[])?)
                .map_err(deferred::invoke_error)?;
            return context
                .higher_order(
                    Combinator::Reducing(transform),
                    &[n, arguments[0].clone(), buffer],
                )
                .map_err(deferred::invoke_error);
        }
        // A reusable transform owns no process state. Each application gets a
        // traced volatile, and zero-arity reducer initialization never resets it.
        let capture = if matches!(transform, Transform::Take | Transform::Drop) {
            context
                .volatile(&captures[0])
                .map_err(deferred::invoke_error)?
        } else {
            captures[0].clone()
        };
        return context
            .higher_order(
                Combinator::Reducing(transform),
                &[capture, arguments[0].clone()],
            )
            .map_err(deferred::invoke_error);
    }
    if !matches!(transform, Transform::Map) && arguments.len() > 2 {
        return Err(InvokeError::Dispatch(EvalErrorKind::Arity {
            fixed: Box::new([0, 1, 2]),
            variadic: None,
            actual: arguments.len(),
        }));
    }
    if matches!(transform, Transform::PartitionAll) {
        return invoke_partition_all(context, captures, arguments);
    }
    if arguments.len() <= 1 {
        return context.invoke(&captures[1], arguments);
    }
    match transform {
        Transform::Map => {
            let mapped = context.invoke(&captures[0], &arguments[1..])?;
            context.invoke(&captures[1], &[arguments[0].clone(), mapped])
        }
        Transform::Filter | Transform::Remove => {
            let selected = context.invoke(&captures[0], &arguments[1..])?;
            if truthy(&selected) == matches!(transform, Transform::Filter) {
                context.invoke(&captures[1], arguments)
            } else {
                Ok(arguments[0].clone())
            }
        }
        Transform::Take | Transform::Drop => {
            use numbers::Operation::{Dec, Positive};
            let n = captures[0]
                .state_value()
                .expect("private transform counter");
            // The pinned reducer decrements before testing/forwarding, even
            // after drop begins forwarding. Keep checked arithmetic and its
            // failure timing; a saturating counter changes observable behavior.
            let next = numbers::call(context, std::slice::from_ref(&n), Dec)
                .map_err(deferred::invoke_error)?;
            state::vreset(context, &[captures[0].clone(), next.clone()])
                .map_err(deferred::invoke_error)?;
            let positive =
                numbers::call(context, &[n], Positive).map_err(deferred::invoke_error)?;
            let result = if truthy(&positive) == matches!(transform, Transform::Take) {
                context.invoke(&captures[1], arguments)?
            } else {
                arguments[0].clone()
            };
            if matches!(transform, Transform::Take)
                && !truthy(
                    &numbers::call(context, &[next], Positive).map_err(deferred::invoke_error)?,
                )
                && result.reduced_value().is_none()
            {
                context.reduced(&result).map_err(deferred::invoke_error)
            } else {
                Ok(result)
            }
        }
        Transform::TakeWhile => {
            let selected = context.invoke(&captures[0], &arguments[1..])?;
            if truthy(&selected) {
                context.invoke(&captures[1], arguments)
            } else {
                context
                    .reduced(&arguments[0])
                    .map_err(deferred::invoke_error)
            }
        }
        Transform::PartitionAll => unreachable!(),
    }
}

fn invoke_partition_all(
    context: &mut Context,
    captures: &[RootedValue],
    arguments: &[RootedValue],
) -> Result<RootedValue, InvokeError> {
    if arguments.is_empty() {
        return context.invoke(&captures[1], arguments);
    }
    let mut buffer = captures[2].state_value().expect("private partition buffer");
    let completing = arguments.len() == 1;
    if !completing {
        buffer = context.conj(&buffer, &arguments[1])?;
        state::vreset(context, &[captures[2].clone(), buffer.clone()])
            .map_err(deferred::invoke_error)?;
    }
    let ValueView::Integer(n) = captures[0].inspect() else {
        unreachable!()
    };
    let count = buffer.count().expect("partition vector");
    let mut result = arguments[0].clone();
    if (completing && count > 0) || (!completing && count == n as usize) {
        // The snapshot roots every input through callback-triggered collection.
        // Clear before calling so reentry and later explicit completion cannot
        // replay the group, even when the downstream callback throws.
        let empty = context.vector(&[])?;
        state::vreset(context, &[captures[2].clone(), empty]).map_err(deferred::invoke_error)?;
        result = context.invoke(&captures[1], &[result, buffer])?;
        if completing {
            result = result.reduced_value().unwrap_or(result);
        }
    }
    if completing {
        context.invoke(&captures[1], &[result])
    } else {
        Ok(result)
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Seq,
    Sequence,
    Concat,
    First,
    Rest,
    Next,
    Cons,
    Reduce,
    Completing,
    Cat,
    Transduce,
    ReduceKv,
    Reduced,
    EnsureReduced,
    ReducedP,
    Unreduced,
    Into,
    Mapv,
    Filterv,
    SearchSome,
    Every,
    NotAny,
    NotEvery,
    Apply,
    Comp,
    Partial,
    Complement,
    Constantly,
    Juxt,
}
fn call(context: &mut Context, args: &[RootedValue], operation: Operation) -> NativeResult {
    use Operation::*;
    match operation {
        Seq => sequence(context, &args[0], 0),
        Sequence => {
            let value = sequence(context, &args[0], 0)?;
            if matches!(value.inspect(), ValueView::Nil) {
                Ok(context.list(&[])?)
            } else {
                Ok(value)
            }
        }
        Concat => context.lazy_concat(args),
        First => Ok(Cursor::new(context, &args[0], 0)?
            .next(context)?
            .unwrap_or_else(|| context.nil())),
        Rest | Next => {
            let source = sequence(context, &args[0], 0)?;
            let tail = context.sequence_tail(&source)?;
            if matches!(operation, Next) {
                sequence(context, &tail, 0)
            } else {
                Ok(tail)
            }
        }
        Cons => {
            if matches!(args[1].inspect(), ValueView::LazySeq) {
                return context.lazy_cons(&args[0], &args[1]);
            }
            let mut tail = sequence(context, &args[1], 1)?;
            if matches!(tail.inspect(), ValueView::Nil)
                && !matches!(args[1].inspect(), ValueView::Nil)
            {
                tail = context.list(&[])?;
            }
            Ok(context.cons_value(&args[0], &tail)?)
        }
        Reduced => context.reduced(&args[0]),
        EnsureReduced => {
            if args[0].reduced_value().is_some() {
                Ok(args[0].clone())
            } else {
                context.reduced(&args[0])
            }
        }
        ReducedP => Ok(context.boolean(args[0].reduced_value().is_some())),
        Unreduced => Ok(args[0].reduced_value().unwrap_or_else(|| args[0].clone())),
        Reduce => reduce(
            context,
            &args[0],
            (args.len() == 3).then(|| &args[1]),
            args.last().unwrap(),
            args.len() - 1,
        ),
        Completing => {
            let completion = args.get(1).cloned().unwrap_or_else(|| {
                context.native_function(
                    builtin_definitions()
                        .iter()
                        .find(|d| d.metadata.name == "identity")
                        .unwrap(),
                )
            });
            context.higher_order(Combinator::Completing, &[args[0].clone(), completion])
        }
        Cat => {
            let preserving = context.higher_order(Combinator::PreservingReduced, args)?;
            context.higher_order(Combinator::Cat, &[args[0].clone(), preserving])
        }
        Transduce => {
            // Initialize the original reducer before applying the transducer.
            // Every local stays rooted across arbitrary reentrant callbacks.
            let initial = if args.len() == 4 {
                args[2].clone()
            } else {
                context.invoke(&args[1], &[])?
            };
            transduce(
                context,
                &args[0],
                &args[1],
                &initial,
                args.last().unwrap(),
                args.len() - 1,
            )
        }
        ReduceKv => {
            let mut accumulator = args[1].clone();
            match args[2].inspect() {
                ValueView::Nil => Ok(accumulator),
                ValueView::Vector(len) if !context.is_map_entry(&args[2]) => {
                    for index in 0..len {
                        poll(context)?;
                        accumulator = context.invoke(
                            &args[0],
                            &[
                                accumulator,
                                context.integer(index as i64),
                                args[2].nth(index)?.unwrap(),
                            ],
                        )?;
                        if let Some(value) = accumulator.reduced_value() {
                            return Ok(value);
                        }
                    }
                    Ok(accumulator)
                }
                ValueView::Map(_) | ValueView::Record(_) => {
                    for (key, value) in args[2].entries()? {
                        poll(context)?;
                        accumulator = context.invoke(&args[0], &[accumulator, key, value])?;
                        if let Some(value) = accumulator.reduced_value() {
                            return Ok(value);
                        }
                    }
                    Ok(accumulator)
                }
                _ => {
                    let mut cursor = Cursor::new(context, &args[2], 2)?;
                    loop {
                        poll(context)?;
                        let Some(entry) = cursor.next(context)? else {
                            break;
                        };
                        if !context.is_map_entry(&entry) {
                            return Err(NativeError::Type {
                                argument: 2,
                                expected: "sequence of map entries",
                            });
                        }
                        accumulator = context.invoke(
                            &args[0],
                            &[accumulator, entry.nth(0)?.unwrap(), entry.nth(1)?.unwrap()],
                        )?;
                        if let Some(value) = accumulator.reduced_value() {
                            return Ok(value);
                        }
                    }
                    Ok(accumulator)
                }
            }
        }
        Into => {
            if args.len() == 3 {
                let conj = context.native_function(
                    collection_core::collection_definitions()
                        .iter()
                        .find(|d| d.metadata.name == "conj")
                        .unwrap(),
                );
                let reducer = call(context, &[conj], Completing)?;
                return transduce(context, &args[1], &reducer, &args[0], &args[2], 2);
            }
            let mut result = args
                .first()
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| context.vector(&[]))?;
            if let Some(source) = args.get(1) {
                let mut cursor = Cursor::new(context, source, 1)?;
                loop {
                    poll(context)?;
                    let Some(value) = cursor.next(context)? else {
                        break;
                    };
                    result = collection_core::conj(context, &result, &value)?;
                }
            }
            Ok(result)
        }
        Mapv => {
            let mut cursors = Vec::with_capacity(args.len() - 1);
            for (i, source) in args[1..].iter().enumerate() {
                let cursor = Cursor::new(context, source, i + 1)?;
                if args.len() > 4 && cursor.is_empty() {
                    return Ok(context.vector(&[])?);
                }
                cursors.push(cursor);
            }
            let mut arguments = Vec::with_capacity(cursors.len());
            let mut result = Vec::new();
            'outer: loop {
                poll(context)?;
                arguments.clear();
                if cursors.iter().any(Cursor::is_empty) {
                    break;
                }
                for cursor in &mut cursors {
                    let Some(value) = cursor.next(context)? else {
                        break 'outer;
                    };
                    arguments.push(value);
                }
                result.push(context.invoke(&args[0], &arguments)?);
            }
            Ok(context.vector(&result)?)
        }
        Filterv | SearchSome | Every | NotAny | NotEvery => {
            let mut cursor = Cursor::new(context, &args[1], 1)?;
            let mut result = Vec::new();
            loop {
                poll(context)?;
                let Some(value) = cursor.next(context)? else {
                    break;
                };
                let selected = context.invoke(&args[0], std::slice::from_ref(&value))?;
                let matches = truthy(&selected);
                match operation {
                    Filterv if matches => result.push(value),
                    SearchSome if matches => return Ok(selected),
                    NotAny if matches => return Ok(context.boolean(false)),
                    Every if !matches => return Ok(context.boolean(false)),
                    NotEvery if !matches => return Ok(context.boolean(true)),
                    _ => {}
                }
            }
            match operation {
                Filterv => Ok(context.vector(&result)?),
                SearchSome => Ok(context.nil()),
                Every | NotAny => Ok(context.boolean(true)),
                NotEvery => Ok(context.boolean(false)),
                _ => unreachable!(),
            }
        }
        Apply => {
            let mut arguments = sequence(context, args.last().unwrap(), args.len() - 1)?;
            for value in args[1..args.len() - 1].iter().rev() {
                poll(context)?;
                arguments = context.cons_value(value, &arguments)?;
            }
            Ok(context.invoke_sequence(&args[0], &arguments)?)
        }
        Comp if args.is_empty() => Ok(context.native_function(
            builtin_definitions()
                .iter()
                .find(|d| d.metadata.name == "identity")
                .unwrap(),
        )),
        Comp | Partial if args.len() == 1 => Ok(args[0].clone()),
        Comp | Partial | Complement | Constantly | Juxt => context.higher_order(
            match operation {
                Comp => Combinator::Comp,
                Partial => Combinator::Partial,
                Complement => Combinator::Complement,
                Constantly => Combinator::Constantly,
                Juxt => Combinator::Juxt,
                _ => unreachable!(),
            },
            args,
        ),
    }
}

/// Both public transducing processes share transformation and completion.
fn transduce(
    context: &mut Context,
    transform: &RootedValue,
    reducer: &RootedValue,
    initial: &RootedValue,
    source: &RootedValue,
    argument: usize,
) -> NativeResult {
    let reducer = context.invoke(transform, std::slice::from_ref(reducer))?;
    let result = reduce(context, &reducer, Some(initial), source, argument)?;
    // Completion is successful reduction's final call, never cleanup.
    // Its result (including Reduced) must be returned unchanged.
    Ok(context.invoke(&reducer, &[result])?)
}

/// Ordinary reduction never completes, and removes exactly one marker layer
/// only from a step result. An untouched initializer is returned as supplied.
pub(super) fn reduce(
    context: &mut Context,
    reducer: &RootedValue,
    initial: Option<&RootedValue>,
    source: &RootedValue,
    argument: usize,
) -> NativeResult {
    let mut cursor = Cursor::new(context, source, argument)?;
    let mut accumulator = if let Some(initial) = initial {
        initial.clone()
    } else if let Some(first) = cursor.next(context)? {
        first
    } else {
        return Ok(context.invoke(reducer, &[])?);
    };
    loop {
        poll(context)?;
        let Some(value) = cursor.next(context)? else {
            return Ok(accumulator);
        };
        accumulator = context.invoke(reducer, &[accumulator, value])?;
        if let Some(value) = accumulator.reduced_value() {
            return Ok(value);
        }
    }
}
macro_rules! def {
    ($name:literal, $arity:expr, $op:ident) => {
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
                ],
            },
            callback: |context, args| call(context, args, Operation::$op),
        }
    };
}
pub fn sequence_definitions() -> &'static [NativeDefinition] {
    use Arity::{AtLeast, Exact, Range};
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("seq", Exact(1), Seq),
        def!("sequence", Exact(1), Sequence),
        def!("concat", AtLeast(0), Concat),
        def!("first", Exact(1), First),
        def!("rest", Exact(1), Rest),
        def!("next", Exact(1), Next),
        def!("cons", Exact(2), Cons),
        def!("reduce", Range { min: 2, max: 3 }, Reduce),
        def!("completing", Range { min: 1, max: 2 }, Completing),
        def!("cat", Exact(1), Cat),
        def!("transduce", Range { min: 3, max: 4 }, Transduce),
        def!("reduce-kv", Exact(3), ReduceKv),
        def!("reduced", Exact(1), Reduced),
        def!("ensure-reduced", Exact(1), EnsureReduced),
        def!("reduced?", Exact(1), ReducedP),
        def!("unreduced", Exact(1), Unreduced),
        def!("into", Range { min: 0, max: 3 }, Into),
        def!("mapv", AtLeast(2), Mapv),
        def!("filterv", Exact(2), Filterv),
        def!("some", Exact(2), SearchSome),
        def!("every?", Exact(2), Every),
        def!("not-any?", Exact(2), NotAny),
        def!("not-every?", Exact(2), NotEvery),
        def!("apply", AtLeast(2), Apply),
        def!("comp", AtLeast(0), Comp),
        def!("partial", AtLeast(1), Partial),
        def!("complement", Exact(1), Complement),
        def!("constantly", Exact(1), Constantly),
        def!("juxt", AtLeast(1), Juxt),
    ];
    DEFINITIONS
}
