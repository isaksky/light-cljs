//! Single-threaded mutable references. All retained edges are unrooted arena
//! values; callback snapshots are rooted only for the duration of an operation.
use super::*;

pub(super) struct Atom {
    pub(super) value: Value,
    pub(super) metadata: Value,
    validator: Value,
    watches: Value,
}
impl Atom {
    pub(super) fn trace(&self, trace: &mut Vec<Target>) {
        trace.extend([self.value, self.metadata, self.validator, self.watches].map(Target::Value));
    }
}
impl RootedValue {
    /// Snapshot an atom/volatile without running code. The returned root remains
    /// valid after later mutation, collection, Context reset or Context drop.
    pub fn state_value(&self) -> Option<RootedValue> {
        let store = self.owner.store.borrow();
        let value = match store.heap_value(self.value())? {
            HeapValue::Atom(atom) => atom.value,
            HeapValue::Volatile(value) => *value,
            _ => return None,
        };
        Some(self.owner.value(value))
    }
}
impl Context {
    /// Construct an atom with no validator, watches or metadata.
    pub fn atom(&self, value: &RootedValue) -> NativeResult {
        self.state_owner(value)?;
        self.charge_allocation(1)?;
        let raw = Value::Heap(HeapId(self.owner.store.borrow_mut().heap.insert(
            HeapValue::Atom(Atom {
                value: value.value(),
                metadata: Value::Nil,
                validator: Value::Nil,
                watches: Value::Nil,
            }),
        )));
        Ok(self.owner.checked_value(raw)?)
    }
    /// Construct a single-threaded volatile reference.
    pub fn volatile(&self, value: &RootedValue) -> NativeResult {
        self.state_owner(value)?;
        self.charge_allocation(1)?;
        let raw = Value::Heap(HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Volatile(value.value())),
        ));
        Ok(self.owner.checked_value(raw)?)
    }
    fn state_owner(&self, value: &RootedValue) -> Result<(), NativeError> {
        if Rc::ptr_eq(&self.owner, &value.owner) {
            Ok(())
        } else {
            Err(NativeError::WrongOwner)
        }
    }
    fn atom_id(&self, value: &RootedValue) -> Result<HeapId, NativeError> {
        self.state_owner(value)?;
        if matches!(value.inspect(), ValueView::Atom) {
            Ok(value.heap_id().unwrap())
        } else {
            Err(state_type(0, "atom"))
        }
    }
    fn atom_field(&self, value: &RootedValue, field: fn(&Atom) -> Value) -> NativeResult {
        let id = self.atom_id(value)?;
        let store = self.owner.store.borrow();
        let HeapValue::Atom(atom) = store.heap.get(id.0).unwrap() else {
            unreachable!()
        };
        Ok(self.owner.value(field(atom)))
    }
    fn update_atom(
        &self,
        value: &RootedValue,
        update: impl FnOnce(&mut Atom),
    ) -> Result<(), NativeError> {
        let id = self.atom_id(value)?;
        self.accounting.check()?;
        let mut store = self.owner.store.borrow_mut();
        let HeapValue::Atom(atom) = store.heap.get_mut(id.0) else {
            unreachable!()
        };
        update(atom);
        Ok(())
    }
    pub(super) fn reset_atom_metadata(
        &self,
        atom: &RootedValue,
        meta: &RootedValue,
    ) -> Result<(), NativeError> {
        self.state_owner(meta)?;
        if !matches!(meta.inspect(), ValueView::Nil | ValueView::Map(_)) {
            return Err(state_type(1, "metadata map or nil"));
        }
        self.update_atom(atom, |atom| atom.metadata = meta.value())
    }
    fn validate_atom(
        &mut self,
        atom: &RootedValue,
        value: &RootedValue,
    ) -> Result<(), NativeError> {
        let validator = self.atom_field(atom, |a| a.validator)?;
        self.validate_state(&validator, value)?;
        // An embedding callback can reset the Context during validation.
        self.state_owner(atom)
    }
    fn validate_state(
        &mut self,
        validator: &RootedValue,
        value: &RootedValue,
    ) -> Result<(), NativeError> {
        if matches!(validator.inspect(), ValueView::Nil) {
            return Ok(());
        }
        let result = match self.invoke(validator, std::slice::from_ref(value)) {
            Ok(result) => result,
            Err(InvokeError::Execution(mut error))
                if error.class() == ErrorClass::Language
                    && error.exception.as_ref().is_some_and(|e| {
                        matches!(e.owner.store.borrow().heap_value(e.value()),
                            Some(HeapValue::Exception(exception)) if exception.checked)
                    }) =>
            {
                // Clojure passes RuntimeException (including ex-info) through,
                // but wraps checked Exception with the rejected-state message.
                let cause = error.exception.take().unwrap();
                let message =
                    self.string("Invalid reference state".encode_utf16().collect::<Vec<_>>());
                let exception = self.runtime_exception(
                    ExceptionCategory::Exception,
                    &message,
                    &self.nil(),
                    &cause,
                )?;
                error.exception = Some(Box::new(exception));
                error.kind = EvalErrorKind::InvalidReferenceState;
                return Err(InvokeError::Execution(error).into());
            }
            Err(error) => return Err(error.into()),
        };
        if matches!(result.inspect(), ValueView::Nil | ValueView::Boolean(false)) {
            return Err(InvokeError::Dispatch(EvalErrorKind::InvalidReferenceState).into());
        }
        Ok(())
    }
    fn notify_watches(
        &mut self,
        atom: &RootedValue,
        old: &RootedValue,
        new: &RootedValue,
    ) -> Result<(), NativeError> {
        // Snapshot after commit. Reentrant watch edits affect the next change,
        // while this immutable map keeps removed callbacks/keys alive for GC.
        let watches = self.atom_field(atom, |a| a.watches)?;
        if matches!(watches.inspect(), ValueView::Nil) {
            return Ok(());
        }
        let count = watches.count().unwrap();
        self.charge_allocation(count as u64 * 2)?;
        self.accounting.work(count as u64)?;
        for (key, function) in watches.entries()? {
            if matches!(function.inspect(), ValueView::Nil) {
                continue;
            }
            self.invoke(&function, &[key, atom.clone(), old.clone(), new.clone()])?;
            self.state_owner(atom)?;
        }
        Ok(())
    }
}
fn callable_or_nil(value: &RootedValue, argument: usize) -> Result<(), NativeError> {
    if matches!(
        value.inspect(),
        ValueView::Nil
            | ValueView::NativeFunction(_)
            | ValueView::Function
            | ValueView::Var(_)
            | ValueView::Keyword(_)
            | ValueView::Symbol(_)
            | ValueView::Map(_)
            | ValueView::Set(_)
            | ValueView::Vector(_)
    ) {
        Ok(())
    } else {
        Err(state_type(argument, "callable or nil"))
    }
}
fn state_type(argument: usize, expected: &'static str) -> NativeError {
    NativeError::Type { argument, expected }
}
fn atom(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    if args.len() == 1 {
        return c.atom(&args[0]);
    }
    let mut meta = c.nil();
    let mut validator = c.nil();
    // The pinned SCI atom wrapper uses apply hash-map, not keyword-arg
    // destructuring: a trailing options map is not accepted.
    if !(args.len() - 1).is_multiple_of(2) {
        return Err(state_type(1, "key/value options"));
    }
    c.charge_allocation(args.len() as u64)?;
    let mut options = c.map(&[])?;
    for pair in args[1..].as_chunks::<2>().0 {
        c.realize_key(&pair[0])?;
        options = c.assoc(&options, &pair[0], &pair[1])?;
    }
    if let Some(value) = options.get(&c.keyword(Name::new(None, "meta")))?
        && !matches!(value.inspect(), ValueView::Boolean(false))
    {
        meta = value;
    }
    if let Some(value) = options.get(&c.keyword(Name::new(None, "validator")))?
        && !matches!(value.inspect(), ValueView::Boolean(false))
    {
        validator = value;
    }
    callable_or_nil(&validator, 1)?;
    let atom = c.atom(&args[0])?;
    c.reset_atom_metadata(&atom, &meta)?;
    c.validate_state(&validator, &args[0])?;
    c.update_atom(&atom, |a| a.validator = validator.value())?;
    Ok(atom)
}
fn reset(c: &mut Context, args: &[RootedValue], pair: bool) -> NativeResult {
    let atom = &args[0];
    let before = c.atom_field(atom, |a| a.value)?;
    c.validate_atom(atom, &args[1])?;
    let old = if pair {
        c.atom_field(atom, |a| a.value)?
    } else {
        before
    };
    c.update_atom(atom, |a| a.value = args[1].value())?;
    c.notify_watches(atom, &old, &args[1])?;
    if pair {
        Ok(c.vector(&[old, args[1].clone()])?)
    } else {
        Ok(args[1].clone())
    }
}
fn swap(c: &mut Context, args: &[RootedValue], pair: bool) -> NativeResult {
    c.atom_id(&args[0])?;
    c.charge_allocation(args.len() as u64 - 1)?;
    let mut params = vec![c.nil()];
    params.extend_from_slice(&args[2..]);
    loop {
        c.accounting.work(1)?;
        let old = c.atom_field(&args[0], |a| a.value)?;
        params[0] = old.clone();
        let new = c.invoke(&args[1], &params)?;
        c.validate_atom(&args[0], &new)?;
        if !old.same_identity(&c.atom_field(&args[0], |a| a.value)?) {
            continue;
        }
        c.update_atom(&args[0], |a| a.value = new.value())?;
        c.notify_watches(&args[0], &old, &new)?;
        return if pair {
            Ok(c.vector(&[old, new])?)
        } else {
            Ok(new)
        };
    }
}
fn compare_set(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    c.validate_atom(&args[0], &args[2])?;
    if !args[1].same_identity(&c.atom_field(&args[0], |a| a.value)?) {
        return Ok(c.boolean(false));
    }
    c.update_atom(&args[0], |a| a.value = args[2].value())?;
    c.notify_watches(&args[0], &args[1], &args[2])?;
    Ok(c.boolean(true))
}
fn watch(c: &mut Context, args: &[RootedValue], remove: bool) -> NativeResult {
    c.atom_id(&args[0])?;
    if !remove {
        callable_or_nil(&args[2], 2)?;
    }
    // ARef snapshots its persistent-map receiver before hashing the key.
    // Lazy key realization may reenter and edit watches; the outer successful
    // update still derives from this original snapshot.
    let watches = c.atom_field(&args[0], |a| a.watches)?;
    if remove
        && (matches!(watches.inspect(), ValueView::Nil)
            || watches.count() == Some(0)
            || (watches.count() == Some(1)
                && watches.get(&c.nil())?.is_some()
                && !matches!(args[1].inspect(), ValueView::Nil)))
    {
        return Ok(args[0].clone());
    }
    c.realize_key(&args[1])?;
    let watches = if remove {
        c.dissoc(&watches, &args[1])?
    } else {
        let watches = if matches!(watches.inspect(), ValueView::Nil) {
            c.map(&[])?
        } else {
            watches
        };
        c.assoc(&watches, &args[1], &args[2])?
    };
    c.update_atom(&args[0], |a| a.watches = watches.value())?;
    Ok(args[0].clone())
}
pub(super) fn vreset(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    c.state_owner(&args[0])?;
    if !matches!(args[0].inspect(), ValueView::Volatile) {
        return Err(state_type(0, "volatile"));
    }
    c.accounting.check()?;
    let mut store = c.owner.store.borrow_mut();
    let HeapValue::Volatile(value) = store.heap.get_mut(args[0].heap_id().unwrap().0) else {
        unreachable!()
    };
    *value = args[1].value();
    Ok(args[1].clone())
}
fn vswap(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    // Preserve pinned SCI's macro: its target expression occurs twice, and the
    // second evaluation supplies the dereferenced value to the function.
    let mut call = vec![
        args[3].clone(),
        binding_macros::call(c, "deref", &args[2..3])?,
    ];
    call.extend_from_slice(&args[4..]);
    binding_macros::call(c, "vreset!", &[args[2].clone(), c.list(&call)?])
}
pub fn state_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $arity:expr, $callback:expr) => {
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
                callback: $callback,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("atom", Arity::AtLeast(1), atom),
        def!("reset!", Arity::Exact(2), |c, a| reset(c, a, false)),
        def!("reset-vals!", Arity::Exact(2), |c, a| reset(c, a, true)),
        def!("swap!", Arity::AtLeast(2), |c, a| swap(c, a, false)),
        def!("swap-vals!", Arity::AtLeast(2), |c, a| swap(c, a, true)),
        def!("compare-and-set!", Arity::Exact(3), compare_set),
        def!("add-watch", Arity::Exact(3), |c, a| watch(c, a, false)),
        def!("remove-watch", Arity::Exact(2), |c, a| watch(c, a, true)),
        def!("get-validator", Arity::Exact(1), |c, a| c
            .atom_field(&a[0], |atom| atom.validator)),
        def!("set-validator!", Arity::Exact(2), |c, a| {
            c.atom_id(&a[0])?;
            callable_or_nil(&a[1], 1)?;
            let value = c.atom_field(&a[0], |atom| atom.value)?;
            c.validate_state(&a[1], &value)?;
            c.update_atom(&a[0], |atom| atom.validator = a[1].value())?;
            Ok(c.nil())
        }),
        def!("volatile!", Arity::Exact(1), |c, a| c.volatile(&a[0])),
        def!("volatile?", Arity::Exact(1), |c, a| Ok(
            c.boolean(matches!(a[0].inspect(), ValueView::Volatile))
        )),
        def!("vreset!", Arity::Exact(2), vreset),
        def!("vswap!", Arity::AtLeast(4), vswap),
    ];
    DEFINITIONS
}
