//! Finite traversal and eager higher-order functions. Callbacks use the common
//! evaluator; every retained edge is traced and no store borrow crosses a call.
use super::*;
use collection_core::{Cursor, sequence, truthy};

#[derive(Clone, Copy)]
pub(super) enum Combinator {
    Comp,
    Partial,
    Complement,
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
        if let Some(source) = applied {
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
    ReduceKv,
    Reduced,
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
        ReducedP => Ok(context.boolean(args[0].reduced_value().is_some())),
        Unreduced => Ok(args[0].reduced_value().unwrap_or_else(|| args[0].clone())),
        Reduce => {
            let mut cursor = Cursor::new(context, args.last().unwrap(), args.len() - 1)?;
            let mut accumulator = if args.len() == 3 {
                args[1].clone()
            } else if let Some(first) = cursor.next(context)? {
                first
            } else {
                return Ok(context.invoke(&args[0], &[])?);
            };
            loop {
                poll(context)?;
                let Some(value) = cursor.next(context)? else {
                    break;
                };
                accumulator = context.invoke(&args[0], &[accumulator, value])?;
                if let Some(value) = accumulator.reduced_value() {
                    return Ok(value);
                }
            }
            Ok(accumulator)
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
                ValueView::Map(_) => {
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
        def!("reduce-kv", Exact(3), ReduceKv),
        def!("reduced", Exact(1), Reduced),
        def!("reduced?", Exact(1), ReducedP),
        def!("unreduced", Exact(1), Unreduced),
        def!("into", Range { min: 0, max: 2 }, Into),
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
