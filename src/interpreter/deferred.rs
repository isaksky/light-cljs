//! Deferred closures and memoized, traced realization. No stored edge owns a
//! RootedValue, Context, source handle, or native callback borrow.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredStatus {
    Unrealized,
    Evaluating,
    Realized,
    Failed,
}

pub(super) struct Deferred {
    pub(super) delay: bool,
    retry: bool,
    state: State,
}
enum State {
    Unrealized(Value),
    Evaluating(Option<Value>),
    Produced(Value),
    Realized(Value),
    // Clojure 1.12 delays cache exceptions. SCI direct lazy syntax consumes its
    // once-only body on invocation; a failed body subsequently yields empty.
    Failed(Option<Box<Failure>>),
    FailedRetry(Value),
}
struct Frame {
    source: SourceId,
    bytes: Range<usize>,
    origin: Origin,
}
struct Failure {
    kind: EvalErrorKind,
    execution: Option<(EvalPhase, Frame, Option<Frame>, Vec<Frame>)>,
    exception: Option<Value>,
}
impl Frame {
    fn save(span: &SourceSpan, origin: Origin) -> Self {
        Self {
            source: span.source().id(),
            bytes: span.bytes(),
            origin,
        }
    }
    fn span(&self, owner: &Rc<Owner>) -> SourceSpan {
        owner
            .source(self.source)
            .expect("traced failure source")
            .span(self.bytes.clone())
            .unwrap()
    }
}
impl Failure {
    fn save(error: &InvokeError) -> Self {
        if let InvokeError::Execution(e) = error
            && let EvalErrorKind::Native {
                error: NativeError::Invocation(inner),
                ..
            } = &e.kind
        {
            return Self::save(inner.error());
        }
        match error {
            InvokeError::Dispatch(EvalErrorKind::Native {
                error: NativeError::Invocation(e),
                ..
            }) => Self::save(e.error()),
            InvokeError::Dispatch(kind) => Self {
                kind: kind.clone(),
                execution: None,
                exception: None,
            },
            InvokeError::Execution(e) => Self {
                kind: e.kind.clone(),
                execution: Some((
                    e.phase,
                    Frame::save(&e.span, e.origin.clone()),
                    e.related.as_ref().map(|s| Frame::save(s, e.origin.clone())),
                    e.frames
                        .iter()
                        .map(|f| Frame::save(&f.span, f.origin.clone()))
                        .collect(),
                )),
                exception: e.exception.as_ref().map(|v| v.value()),
            },
        }
    }
    fn restore(&self, owner: &Rc<Owner>) -> InvokeError {
        match &self.execution {
            None => InvokeError::Dispatch(self.kind.clone()),
            Some((phase, frame, related, frames)) => InvokeError::Execution(Box::new(EvalError {
                phase: *phase,
                kind: self.kind.clone(),
                span: Box::new(frame.span(owner)),
                related: related.as_ref().map(|f| Box::new(f.span(owner))),
                origin: frame.origin.clone(),
                frames: frames
                    .iter()
                    .map(|f| SourceFrame {
                        span: f.span(owner),
                        origin: f.origin.clone(),
                    })
                    .collect(),
                exception: self.exception.map(|v| Box::new(owner.value(v))),
            })),
        }
    }
    fn trace(&self, trace: &mut Vec<Target>) {
        trace.extend(self.exception.map(Target::Value));
        if let Some((_, frame, related, frames)) = &self.execution {
            trace.push(Target::Source(frame.source));
            trace.extend(
                related
                    .iter()
                    .chain(frames)
                    .map(|f| Target::Source(f.source)),
            );
        }
    }
}
impl Deferred {
    pub(super) fn new(thunk: Value, delay: bool, retry: bool) -> Self {
        Self {
            delay,
            retry,
            state: State::Unrealized(thunk),
        }
    }
    pub(super) fn realized_value(&self) -> Option<Value> {
        if self.delay {
            return None;
        }
        match self.state {
            State::Realized(v) => Some(v),
            _ => None,
        }
    }
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        match &self.state {
            State::Unrealized(v)
            | State::FailedRetry(v)
            | State::Produced(v)
            | State::Realized(v) => trace.push(Target::Value(*v)),
            State::Failed(Some(e)) => e.trace(trace),
            State::Evaluating(Some(v)) => trace.push(Target::Value(*v)),
            _ => (),
        }
    }
}
impl RootedValue {
    /// Inspection does not execute deferred code, including failed lazy bodies.
    pub fn deferred_status(&self) -> Option<DeferredStatus> {
        let store = self.owner.store.borrow();
        let HeapValue::Deferred(d) = store.heap_value(self.value())? else {
            return None;
        };
        Some(match d.state {
            State::Unrealized(_) | State::Produced(_) => DeferredStatus::Unrealized,
            State::Evaluating(_) => DeferredStatus::Evaluating,
            State::Realized(_) => DeferredStatus::Realized,
            State::Failed(_) | State::FailedRetry(_) => DeferredStatus::Failed,
        })
    }
}
impl Context {
    // Root construction is intentionally infallible and latches exhaustion.
    // Every public evaluation boundary must check it after success or failure.
    fn finish_deferred<T>(&self, result: Result<T, NativeError>) -> Result<T, NativeError> {
        if !matches!(&result, Err(NativeError::Invocation(e)) if e.error().class() == ErrorClass::Control)
        {
            self.accounting
                .check()
                .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        }
        result
    }
    pub(super) fn deferred(&self, function: &RootedValue, delay: bool) -> NativeResult {
        self.deferred_retry(function, delay, false)
    }
    pub(super) fn deferred_retry(
        &self,
        function: &RootedValue,
        delay: bool,
        retry: bool,
    ) -> NativeResult {
        self.charge_allocation(1)?;
        if !Rc::ptr_eq(&self.owner, &function.owner) {
            return Err(NativeError::WrongOwner);
        }
        let raw = Value::Heap(HeapId(self.owner.store.borrow_mut().heap.insert(
            HeapValue::Deferred(Deferred {
                delay,
                retry,
                state: State::Unrealized(function.value()),
            }),
        )));
        Ok(self.owner.checked_value(raw)?)
    }
    fn deferred_state(&self, id: HeapId, state: State) {
        let mut store = self.owner.store.borrow_mut();
        let HeapValue::Deferred(d) = store.heap.get_mut(id.0) else {
            unreachable!("rooted deferred value")
        };
        d.state = state;
    }
    /// Force a delay through the active evaluator. Other values are unchanged.
    pub fn force(&mut self, value: &RootedValue) -> NativeResult {
        let _scope = self.accounting.enter();
        let result = self.force_inner(value);
        self.finish_deferred(result)
    }
    fn force_inner(&mut self, value: &RootedValue) -> NativeResult {
        self.poll_execution(PollPoint::Function)
            .map_err(InvokeError::Dispatch)?;
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        if !matches!(value.inspect(), ValueView::Delay) {
            return Ok(value.clone());
        }
        let (id, thunk) = {
            let store = self.owner.store.borrow();
            let Value::Heap(id) = store.unannotated(value.value()) else {
                unreachable!()
            };
            let Some(HeapValue::Deferred(d)) = store.heap_value(value.value()) else {
                unreachable!()
            };
            match &d.state {
                State::Realized(v) => return Ok(self.owner.value(*v)),
                State::Failed(Some(e)) => return Err(e.restore(&self.owner).into()),
                State::Evaluating(Some(v)) | State::Unrealized(v) => (id, self.owner.value(*v)),
                _ => unreachable!(),
            }
        };
        self.deferred_state(id, State::Evaluating(Some(thunk.value())));
        let mut result = self.invoke(&thunk, &[]);
        if let Err(InvokeError::Execution(e)) = &mut result
            && e.class() == ErrorClass::Language
            && e.exception.is_none()
        {
            let message = self.string(e.kind.to_string().encode_utf16().collect::<Vec<_>>());
            match self.runtime_exception(
                ExceptionCategory::Exception,
                &message,
                &self.nil(),
                &self.nil(),
            ) {
                Ok(exception) => e.exception = Some(Box::new(exception)),
                Err(error) => result = Err(invoke_error(error)),
            }
        }
        // A callback may reset the context; never dereference an old ID in the new owner.
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        match &result {
            Ok(v) => self.deferred_state(id, State::Realized(v.value())),
            Err(e) => self.deferred_state(id, State::Failed(Some(Box::new(Failure::save(e))))),
        }
        result.map_err(Into::into)
    }
    /// Obtain a sequence, realizing only enough deferred bodies to establish its
    /// head. Consecutive lazy wrappers use a loop, not recursive host calls.
    pub fn seq(&mut self, value: &RootedValue) -> NativeResult {
        let _scope = self.accounting.enter();
        let result = if !Rc::ptr_eq(&self.owner, &value.owner) {
            Err(NativeError::WrongOwner)
        } else {
            collection_core::sequence(self, value, 0)
        };
        self.finish_deferred(result)
    }
    pub(super) fn realize_lazy(&mut self, value: &RootedValue) -> NativeResult {
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        let mut current = value.clone();
        let mut pending = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let outcome = (|| {
            loop {
                self.poll_execution(PollPoint::Iteration)
                    .map_err(InvokeError::Dispatch)?;
                let action = {
                    let store = self.owner.store.borrow();
                    match store.heap_value(current.value()) {
                        Some(HeapValue::Deferred(d)) if !d.delay => {
                            let Value::Heap(id) = store.unannotated(current.value()) else {
                                unreachable!()
                            };
                            match &d.state {
                                State::Realized(v) => return Ok(self.owner.value(*v)),
                                State::Produced(v) => (Some(id), None, Some(self.owner.value(*v))),
                                State::Unrealized(v) | State::FailedRetry(v) => {
                                    (Some(id), Some(self.owner.value(*v)), None)
                                }
                                State::Failed(_) => (Some(id), None, Some(self.nil())),
                                State::Evaluating(_) => {
                                    drop(store);
                                    self.deferred_state(id, State::Realized(Value::Nil));
                                    return Ok(self.nil());
                                }
                            }
                        }
                        _ => (None, None, None),
                    }
                };
                match action {
                    (Some(id), thunk, next) => {
                        if !seen.insert(id) {
                            return Ok(self.nil());
                        }
                        self.charge_allocation(2)?;
                        self.deferred_state(
                            id,
                            State::Evaluating(thunk.as_ref().map(RootedValue::value)),
                        );
                        pending.push((id, current));
                        current = if let Some(thunk) = thunk {
                            self.invoke(&thunk, &[])?
                        } else {
                            next.unwrap()
                        };
                        if !Rc::ptr_eq(&self.owner, &value.owner) {
                            return Err(NativeError::WrongOwner);
                        }
                        self.deferred_state(id, State::Produced(current.value()));
                    }
                    _ => return collection_core::sequence_ready(self, &current, 0),
                }
            }
        })();
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        for (index, (id, _)) in pending.iter().enumerate() {
            match &outcome {
                Ok(v) if index == 0 => self.deferred_state(*id, State::Realized(v.value())),
                Ok(v) => self.deferred_state(*id, State::Produced(v.value())),
                Err(_) => {
                    let state = {
                        let store = self.owner.store.borrow();
                        let HeapValue::Deferred(d) = store.heap.get(id.0).unwrap() else {
                            unreachable!()
                        };
                        match d.state {
                            State::Evaluating(Some(thunk)) if d.retry => {
                                Some(State::FailedRetry(thunk))
                            }
                            State::Evaluating(_) => Some(State::Failed(None)),
                            _ if index == 0 => Some(State::Failed(None)),
                            _ => None,
                        }
                    };
                    if let Some(state) = state {
                        self.deferred_state(*id, state);
                    }
                }
            }
        }
        outcome
    }
    /// Realize and root a finite sequence's members. Infinite values require a
    /// finite consumer (first/next/nth/reduced) or configured fuel/cancellation.
    pub fn sequence_elements(
        &mut self,
        value: &RootedValue,
    ) -> Result<Vec<RootedValue>, NativeError> {
        let _scope = self.accounting.enter();
        let result = self.sequence_elements_inner(value);
        self.finish_deferred(result)
    }
    fn sequence_elements_inner(
        &mut self,
        value: &RootedValue,
    ) -> Result<Vec<RootedValue>, NativeError> {
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(NativeError::WrongOwner);
        }
        let mut cursor = collection_core::Cursor::new(self, value, 0)?;
        let mut result = Vec::new();
        while let Some(v) = cursor.next(self)? {
            self.charge_allocation(1)?;
            result.push(v);
        }
        Ok(result)
    }
    pub(super) fn lazy_cons(&self, head: &RootedValue, tail: &RootedValue) -> NativeResult {
        Ok(self.cons_value(head, tail)?)
    }
}
fn deferred_macro(c: &mut Context, a: &[RootedValue], delay: bool) -> NativeResult {
    let mut body = vec![c.symbol(Name::new(None, "fn*")), c.vector(&[])?];
    body.extend_from_slice(&a[2..]);
    Ok(c.list(&[
        c.symbol(Name::new(
            Some("clojure.core"),
            if delay {
                "delay*"
            } else if c.macros.analysis_expansion {
                "lazy-seq-once*"
            } else {
                "lazy-seq*"
            },
        )),
        c.list(&body)?,
    ])?)
}
pub(super) fn delay_macro(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    deferred_macro(c, a, true)
}
pub(super) fn lazy_macro(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    deferred_macro(c, a, false)
}
pub fn deferred_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $callback:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: Arity::Exact(1),
                    effects: NativeEffects::Context,
                    errors: &[
                        NativeErrorClass::Arity,
                        NativeErrorClass::Type,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: $callback,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("delay*", |c, a| c.deferred(&a[0], true)),
        def!("lazy-seq*", |c, a| c.deferred_retry(&a[0], false, true)),
        def!("lazy-seq-once*", |c, a| c.deferred(&a[0], false)),
        def!("force", |c, a| c.force(&a[0])),
        def!("delay?", |c, a| Ok(
            c.boolean(matches!(a[0].inspect(), ValueView::Delay))
        )),
        def!("realized?", |c, a| {
            let status = a[0].deferred_status().ok_or(NativeError::Type {
                argument: 0,
                expected: "delay or lazy sequence",
            })?;
            Ok(c.boolean(
                status == DeferredStatus::Realized
                    || status == DeferredStatus::Failed
                        && matches!(a[0].inspect(), ValueView::Delay),
            ))
        }),
    ];
    DEFINITIONS
}

impl Context {
    /// Language equality may execute lazy producers; the immutable handle API
    /// reports RequiresRealization instead of executing without a Context.
    pub fn value_eq(
        &mut self,
        left: &RootedValue,
        right: &RootedValue,
    ) -> Result<bool, NativeError> {
        let _scope = self.accounting.enter();
        let result = self.value_eq_inner(left, right);
        self.finish_deferred(result)
    }
    fn value_eq_inner(
        &mut self,
        left: &RootedValue,
        right: &RootedValue,
    ) -> Result<bool, NativeError> {
        if !Rc::ptr_eq(&self.owner, &left.owner) || !Rc::ptr_eq(&self.owner, &right.owner) {
            return Err(NativeError::WrongOwner);
        }
        enum Task {
            Pair(RootedValue, RootedValue),
            Seq(collection_core::Cursor, collection_core::Cursor),
        }
        let sequential = |v: &ValueView| {
            matches!(
                v,
                ValueView::List(_) | ValueView::Vector(_) | ValueView::Seq(_) | ValueView::LazySeq
            )
        };
        let mut tasks = vec![Task::Pair(left.clone(), right.clone())];
        while let Some(task) = tasks.pop() {
            self.poll_execution(PollPoint::Iteration)
                .map_err(InvokeError::Dispatch)?;
            match task {
                Task::Seq(mut a, mut b) => {
                    a.establish(self)?;
                    b.establish(self)?;
                    if a.is_empty() || b.is_empty() {
                        if a.is_empty() != b.is_empty() {
                            return Ok(false);
                        }
                        continue;
                    }
                    let x = a.next(self)?;
                    let y = b.next(self)?;
                    match (x, y) {
                        (Some(x), Some(y)) => {
                            tasks.push(Task::Seq(a, b));
                            tasks.push(Task::Pair(x, y));
                        }
                        (None, None) => (),
                        _ => return Ok(false),
                    }
                }
                Task::Pair(a, b) => {
                    // Metadata wrappers do not change collection identity. This
                    // also keeps a record containing NaN reflexive after with-meta,
                    // consistently with the eager equality/hash implementation.
                    let same_heap = {
                        let store = self.owner.store.borrow();
                        matches!((store.unannotated(a.value()), store.unannotated(b.value())),
                            (Value::Heap(x), Value::Heap(y)) if x == y)
                    };
                    if same_heap {
                        continue;
                    }
                    match (a.inspect(), b.inspect()) {
                        (x, y) if sequential(&x) && sequential(&y) => tasks.push(Task::Seq(
                            collection_core::Cursor::new(self, &a, 0)?,
                            collection_core::Cursor::new(self, &b, 1)?,
                        )),
                        (ValueView::Map(n), ValueView::Map(m))
                        | (ValueView::Record(n), ValueView::Record(m)) => {
                            if records::type_id(&a) != records::type_id(&b) {
                                return Ok(false);
                            }
                            if n != m {
                                return Ok(false);
                            }
                            let entries = a.entries()?;
                            for (key, value) in entries.into_iter().rev() {
                                let Some(other) = b.get(&key)? else {
                                    return Ok(false);
                                };
                                tasks.push(Task::Pair(value, other));
                            }
                        }
                        _ => {
                            if !a.try_value_eq(&b)? {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
        }
        Ok(true)
    }
    /// Hashing a collection key must demand all of its sequential components.
    /// Walk iteratively so nested keys do not grow the host call stack.
    pub(super) fn realize_key(&mut self, value: &RootedValue) -> Result<(), NativeError> {
        let mut pending = vec![value.clone()];
        let mut seen = std::collections::HashSet::new();
        while let Some(value) = pending.pop() {
            self.poll_execution(PollPoint::Iteration)
                .map_err(InvokeError::Dispatch)?;
            if let Some(id) = value.heap_id()
                && !seen.insert(id)
            {
                continue;
            }
            match value.inspect() {
                ValueView::LazySeq => pending.extend(self.sequence_elements(&value)?),
                ValueView::List(_)
                | ValueView::Seq(_)
                | ValueView::Vector(_)
                | ValueView::Set(_) => pending.extend(value.elements()?),
                ValueView::Map(_) | ValueView::Record(_) => {
                    pending.extend(value.entries()?.into_iter().flat_map(|(k, v)| [k, v]))
                }
                _ => (),
            }
        }
        Ok(())
    }
}

pub(super) fn invoke_error(error: NativeError) -> InvokeError {
    match error {
        NativeError::Invocation(error) => error.into_error(),
        error => InvokeError::Dispatch(EvalErrorKind::Native {
            name: Box::new(Name::new(Some("clojure.core"), "lazy-seq")),
            error,
        }),
    }
}
impl Context {
    pub(super) fn lazy_concat(&self, sources: &[RootedValue]) -> NativeResult {
        if sources.len() > 2 {
            let mut grouped = vec![self.lazy_concat(&sources[..2])?];
            grouped.extend_from_slice(&sources[2..]);
            return self.lazy_cat(&grouped);
        }
        let body = self.higher_order(sequence_core::Combinator::LazyConcat, sources)?;
        self.deferred_retry(&body, false, true)
    }
    fn lazy_cat(&self, sources: &[RootedValue]) -> NativeResult {
        let body = self.higher_order(sequence_core::Combinator::LazyCat, sources)?;
        self.deferred_retry(&body, false, true)
    }
    pub(super) fn lazy_projection(&self, source: &RootedValue, index: usize) -> NativeResult {
        let body = self.higher_order(
            if index == 0 {
                sequence_core::Combinator::LazyKeys
            } else {
                sequence_core::Combinator::LazyVals
            },
            std::slice::from_ref(source),
        )?;
        self.deferred_retry(&body, false, true)
    }
}
pub(super) fn produce_concat(
    c: &mut Context,
    callee: &RootedValue,
    sources: &[RootedValue],
    cat: bool,
) -> NativeResult {
    for (index, source) in sources.iter().enumerate() {
        let seq = match c.seq(source) {
            Ok(seq) => seq,
            Err(error) => {
                // Host concat consumes the current collection capture even if
                // seq throws; another demand continues with the next source.
                if Rc::ptr_eq(&c.owner, &callee.owner) {
                    let mut store = c.owner.store.borrow_mut();
                    let Value::Heap(id) = store.unannotated(callee.value()) else {
                        unreachable!()
                    };
                    let HeapValue::HigherOrder(function) = store.heap.get_mut(id.0) else {
                        unreachable!()
                    };
                    function.captures = sources[index + 1..]
                        .iter()
                        .map(RootedValue::value)
                        .collect();
                }
                return Err(error);
            }
        };
        if matches!(seq.inspect(), ValueView::Nil) {
            continue;
        }
        if index + 1 == sources.len() && !cat {
            return Ok(seq);
        }
        let head = seq.nth(0)?.expect("nonempty sequence");
        let mut rest = vec![c.sequence_tail(&seq)?];
        rest.extend_from_slice(&sources[index + 1..]);
        let tail = if cat {
            c.lazy_cat(&rest)?
        } else {
            c.lazy_concat(&rest)?
        };
        return c.lazy_cons(&head, &tail);
    }
    Ok(c.nil())
}
pub(super) fn produce_projection(
    c: &mut Context,
    source: &RootedValue,
    index: usize,
) -> NativeResult {
    let seq = c.seq(source)?;
    if matches!(seq.inspect(), ValueView::Nil) {
        return Ok(c.nil());
    }
    let entry = seq.nth(0)?.expect("nonempty sequence");
    if !c.is_map_entry(&entry) {
        return Err(CollectionError::ExpectedMapEntry.into());
    }
    let head = entry.nth(index)?.unwrap();
    let tail = c.lazy_projection(&c.sequence_tail(&seq)?, index)?;
    c.lazy_cons(&head, &tail)
}
