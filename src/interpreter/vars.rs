//! Var roots and context-local dynamic bindings are separate traced edges.
//! Cleanup guards own their old owner, never borrow Context across callbacks.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VarError {
    Handle(HandleError),
    Resource(ResourceError),
    PermissionDenied(Name),
    NonDynamic,
    NoBinding,
    ReadOnly,
    ExpectedNamespace,
    ExpectedMetadata,
}
impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadOnly => f.write_str("Built-in var is read-only"),
            Self::NonDynamic => f.write_str("Can't dynamically bind non-dynamic var"),
            Self::NoBinding => f.write_str("Can't change/establish root binding with set!"),
            other => write!(f, "{other:?}"),
        }
    }
}
impl std::error::Error for VarError {}
impl From<HandleError> for VarError {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}
impl From<ResourceError> for VarError {
    fn from(value: ResourceError) -> Self {
        Self::Resource(value)
    }
}
impl VarError {
    pub(super) fn kind(self) -> EvalErrorKind {
        match self {
            Self::PermissionDenied(name) => EvalErrorKind::PermissionDenied(name),
            Self::Resource(error) => EvalErrorKind::Resource(error),
            Self::Handle(HandleError::WrongOwner) => EvalErrorKind::WrongOwner,
            other => EvalErrorKind::Var(other),
        }
    }
}
impl From<VarError> for NativeError {
    fn from(value: VarError) -> Self {
        InvokeError::Dispatch(value.kind()).into()
    }
}

/// Owns a dynamic scope without borrowing Context. Drop restores even on Rust
/// errors/panics or after Context::reset. Guards may safely be dropped out of order.
#[must_use = "dropping this guard restores the previous dynamic bindings"]
pub struct BindingGuard {
    vars: Vec<RootedValue>,
    token: Rc<()>,
}
impl Drop for BindingGuard {
    fn drop(&mut self) {
        for var in &self.vars {
            let mut store = var.owner.store.borrow_mut();
            let bindings = &mut store.vars.get_mut(var.var_id().unwrap().0).bindings;
            if bindings
                .last()
                .is_some_and(|(token, _)| Rc::ptr_eq(token, &self.token))
            {
                bindings.pop();
            } else {
                bindings.retain(|(token, _)| !Rc::ptr_eq(token, &self.token));
            }
        }
    }
}
struct RedefinitionGuard {
    roots: Vec<(RootedValue, Option<RootedValue>)>,
}
impl Drop for RedefinitionGuard {
    fn drop(&mut self) {
        for (var, root) in &self.roots {
            var.owner
                .store
                .borrow_mut()
                .vars
                .get_mut(var.var_id().unwrap().0)
                .root = root.as_ref().map(RootedValue::value);
        }
    }
}

impl Context {
    /// Explicit context-only permission for language mutation of builtin roots
    /// and metadata. Reset clears it; independent contexts never inherit it.
    pub fn allow_builtin_var_mutation(&mut self, allow: bool) {
        self.builtin_var_mutation.set(allow);
    }
    pub(super) fn check_var_write(&self, id: VarId) -> Result<(), VarError> {
        self.check_var_access(id).map_err(|e| match e {
            EvalErrorKind::PermissionDenied(name) => VarError::PermissionDenied(name),
            _ => VarError::Handle(HandleError::WrongOwner),
        })?;
        let id = self.context_var(id)?;
        let store = self.owner.store.borrow();
        let var = store.vars.get(id.0)?;
        let builtin = var.builtin;
        let metadata = var.metadata;
        drop(store);
        let marked = self
            .owner
            .value(metadata)
            .get(&self.keyword(Name::new(Some("sci"), "built-in")))
            .ok()
            .flatten()
            .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false)));
        self.accounting.check()?;
        if (builtin || marked) && !self.builtin_var_mutation.get() {
            Err(VarError::ReadOnly)
        } else {
            Ok(())
        }
    }
    pub fn var_is_dynamic(&self, id: VarId) -> Result<bool, HandleError> {
        let id = self.context_var(id)?;
        let meta = self.owner.store.borrow().vars.get(id.0)?.metadata;
        let meta = self.owner.value(meta);
        Ok(meta
            .get(&self.keyword(Name::new(None, "dynamic")))
            .ok()
            .flatten()
            .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false))))
    }
    pub fn var_is_thread_bound(&self, id: VarId) -> Result<bool, HandleError> {
        let id = self.context_var(id)?;
        Ok(!self
            .owner
            .store
            .borrow()
            .vars
            .get(id.0)?
            .bindings
            .is_empty())
    }
    /// Read the active binding, falling back to the raw root (including unbound).
    pub fn var_get(&self, id: VarId) -> Result<RootedValue, HandleError> {
        let id = self.context_var(id)?;
        let value = {
            let store = self.owner.store.borrow();
            let var = store.vars.get(id.0)?;
            var.bindings
                .last()
                .map(|(_, v)| *v)
                .or(var.root)
                .unwrap_or(Value::Unbound(id))
        };
        Ok(self.owner.value(value))
    }
    fn validate_binding_value(&self, id: VarId, value: &RootedValue) -> Result<(), VarError> {
        if !Rc::ptr_eq(&self.owner, &value.owner) {
            return Err(HandleError::WrongOwner.into());
        }
        let store = self.owner.store.borrow();
        let var = store.vars.get(id.0)?;
        if var.namespace == self.registry().borrow().core
            && store
                .names
                .get(var.name.0)?
                .name()
                .iter()
                .copied()
                .eq("*ns*".encode_utf16())
            && value.namespace_id().is_none()
        {
            return Err(VarError::ExpectedNamespace);
        }
        Ok(())
    }
    /// Bind already evaluated values atomically. Each Var must be dynamic;
    /// builtin dynamics can be bound without permitting root/meta mutation.
    pub fn bind_vars(&self, bindings: &[(VarId, RootedValue)]) -> Result<BindingGuard, VarError> {
        let bindings = bindings
            .iter()
            .map(|(id, v)| Ok((self.context_var(*id)?, v.clone())))
            .collect::<Result<Vec<_>, VarError>>()?;
        let bindings = bindings.as_slice();
        self.accounting.work(bindings.len() as u64)?;
        self.charge_allocation(bindings.len() as u64 + 1)?;
        for (id, value) in bindings {
            self.validate_binding_value(*id, value)?;
            let dynamic = self.var_is_dynamic(*id)?;
            self.accounting.check()?;
            if !dynamic {
                return Err(VarError::NonDynamic);
            }
        }
        let mut guard = BindingGuard {
            vars: Vec::with_capacity(bindings.len()),
            token: Rc::new(()),
        };
        for (id, value) in bindings {
            guard.vars.push(self.var_value(*id)?);
            let mut store = self.owner.store.borrow_mut();
            let slots = &mut store.vars.get_mut(id.0).bindings;
            if let Some((_, old)) = slots
                .last_mut()
                .filter(|(t, _)| Rc::ptr_eq(t, &guard.token))
            {
                *old = value.value();
            } else {
                slots.push((guard.token.clone(), value.value()));
            }
        }
        self.accounting.check()?;
        Ok(guard)
    }
    /// Assign an existing dynamic slot. Never creates or changes a root.
    pub fn set_var_binding(&mut self, id: VarId, value: &RootedValue) -> Result<(), VarError> {
        self.check_var_access(id).map_err(|e| match e {
            EvalErrorKind::PermissionDenied(name) => VarError::PermissionDenied(name),
            _ => VarError::Handle(HandleError::WrongOwner),
        })?;
        let id = self.context_var(id)?;
        self.validate_binding_value(id, value)?;
        let mut store = self.owner.store.borrow_mut();
        let (_, binding) = store
            .vars
            .get_mut(id.0)
            .bindings
            .last_mut()
            .ok_or(VarError::NoBinding)?;
        *binding = value.value();
        Ok(())
    }
    /// Checked metadata mutation; removing :dynamic affects subsequent binds.
    pub fn reset_var_metadata(
        &mut self,
        id: VarId,
        metadata: &RootedValue,
    ) -> Result<(), VarError> {
        let id = self.context_var(id)?;
        if !Rc::ptr_eq(&self.owner, &metadata.owner) {
            return Err(HandleError::WrongOwner.into());
        }
        self.check_var_write(id)?;
        if !matches!(metadata.inspect(), ValueView::Nil | ValueView::Map(_)) {
            return Err(VarError::ExpectedMetadata);
        }
        self.set_var_metadata(id, metadata);
        Ok(())
    }
    /// Root redefinition scope. Values are checked before changing any root;
    /// saved roots and Var identities stay alive even if unmapped during body.
    pub fn with_var_redefs<T>(
        &mut self,
        bindings: &[(VarId, RootedValue)],
        body: impl FnOnce(&mut Self) -> T,
    ) -> Result<T, VarError> {
        let bindings = bindings
            .iter()
            .map(|(id, v)| Ok((self.context_var(*id)?, v.clone())))
            .collect::<Result<Vec<_>, VarError>>()?;
        let bindings = bindings.as_slice();
        self.accounting.work(bindings.len() as u64)?;
        self.charge_allocation(bindings.len() as u64 + 1)?;
        let mut guard = RedefinitionGuard {
            roots: Vec::with_capacity(bindings.len()),
        };
        for (id, value) in bindings {
            self.validate_binding_value(*id, value)?;
            self.check_var_write(*id)?;
            if !guard.roots.iter().any(|(v, _)| v.var_id() == Some(*id)) {
                let old = self.owner.store.borrow().vars.get(id.0)?.root;
                guard
                    .roots
                    .push((self.var_value(*id)?, old.map(|v| self.owner.value(v))));
            }
        }
        self.accounting.check()?;
        for (id, value) in bindings {
            self.owner.store.borrow_mut().vars.get_mut(id.0).root = Some(value.value());
        }
        Ok(body(self))
    }
}

fn var(args: &[RootedValue], i: usize) -> Result<VarId, NativeError> {
    args[i].var_id().ok_or(NativeError::Type {
        argument: i,
        expected: "Var",
    })
}
fn binding_map(c: &Context, value: &RootedValue) -> Result<Vec<(VarId, RootedValue)>, NativeError> {
    if !matches!(value.inspect(), ValueView::Nil | ValueView::Map(_)) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "binding map",
        });
    }
    if matches!(value.inspect(), ValueView::Nil) {
        return Ok(Vec::new());
    }
    c.accounting.work(value.count().unwrap_or(0) as u64)?;
    c.charge_allocation(value.count().unwrap_or(0) as u64)?;
    value
        .entries()?
        .into_iter()
        .map(|(v, value)| {
            Ok((
                v.var_id().ok_or(NativeError::Type {
                    argument: 0,
                    expected: "Var keys",
                })?,
                value,
            ))
        })
        .collect()
}
fn with_bindings(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let bindings = binding_map(c, &args[0])?;
    for (id, _) in &bindings {
        c.check_var_access(*id).map_err(InvokeError::Dispatch)?;
    }
    let _guard = c.bind_vars(&bindings)?;
    c.invoke(&args[1], &args[2..]).map_err(Into::into)
}
fn with_redefs(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let bindings = binding_map(c, &args[0])?;
    c.with_var_redefs(&bindings, |c| c.invoke(&args[1], &[]))?
        .map_err(Into::into)
}
fn alter_root(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let id = var(args, 0)?;
    c.check_var_access(id).map_err(InvokeError::Dispatch)?;
    let mut params = vec![c.var_root(id).map_err(VarError::from)?];
    c.charge_allocation(args.len() as u64)?;
    params.extend_from_slice(&args[2..]);
    let value = c.invoke(&args[1], &params)?;
    // SCI evaluates the function before checking builtin write permission.
    c.check_var_write(id)?;
    c.validate_binding_value(id, &value)?;
    c.set_var_root_internal(id, &value)
        .map_err(VarError::from)?;
    Ok(value)
}
fn alter_meta(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let id = if matches!(args[0].inspect(), ValueView::Atom) {
        None
    } else {
        let id = var(args, 0)?;
        c.check_var_write(id)?;
        Some(id)
    };
    c.charge_allocation(args.len() as u64)?;
    let metadata = match id {
        Some(id) => c.var_value(id).map_err(VarError::from)?.metadata(),
        None => args[0].metadata(),
    };
    let mut params = vec![metadata.unwrap_or(c.nil())];
    params.extend_from_slice(&args[2..]);
    let value = c.invoke(&args[1], &params)?;
    if let Some(id) = id {
        c.reset_var_metadata(id, &value)?;
    } else {
        c.reset_atom_metadata(&args[0], &value)?;
    }
    Ok(value)
}
fn reset_meta(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    if matches!(args[0].inspect(), ValueView::Atom) {
        c.reset_atom_metadata(&args[0], &args[1])?;
    } else {
        c.reset_var_metadata(var(args, 0)?, &args[1])?;
    }
    Ok(args[1].clone())
}

pub(super) fn scope_macro(
    c: &mut Context,
    args: &[RootedValue],
    redefs: bool,
    map: bool,
) -> NativeResult {
    use binding_macros::call;
    let bindings = if map {
        args[2].clone()
    } else {
        if !matches!(args[2].inspect(), ValueView::Vector(_))
            && !(redefs
                && matches!(
                    args[2].inspect(),
                    ValueView::List(_) | ValueView::Nil | ValueView::Seq(_)
                ))
        {
            return Err(binding_macros::invalid("binding requires a vector"));
        }
        let values = if matches!(args[2].inspect(), ValueView::Nil) {
            Vec::new()
        } else {
            args[2].elements()?
        };
        if !redefs && !values.len().is_multiple_of(2) {
            return Err(binding_macros::invalid(
                "binding vector requires an even number of forms",
            ));
        }
        let mut pairs = Vec::with_capacity(values.len() / 2);
        for pair in values.as_chunks::<2>().0 {
            if !matches!(pair[0].inspect(), ValueView::Symbol(_)) {
                return Err(binding_macros::invalid("binding target must be a symbol"));
            }
            // Var is a primitive, so use an unqualified operator.
            let key = c.list(&[c.symbol(Name::new(None, "var")), pair[0].clone()])?;
            pairs.push((key, pair[1].clone()));
        }
        if redefs {
            c.map(&pairs)?
        } else {
            let entries: Vec<_> = pairs.into_iter().flat_map(|(k, v)| [k, v]).collect();
            call(c, "hash-map", &entries)?
        }
    };
    let mut function = vec![c.vector(&[])?];
    if !redefs && !map {
        // binding introduces a try boundary: recur cannot escape its cleanup.
        let mut body = vec![c.symbol(Name::new(None, "try"))];
        body.extend_from_slice(&args[3..]);
        body.push(c.list(&[c.symbol(Name::new(None, "finally")), c.nil()])?);
        function.push(c.list(&body)?);
    } else {
        function.extend_from_slice(&args[3..]);
    }
    let function = call(c, "fn*", &function)?;
    call(
        c,
        if redefs {
            "with-redefs-fn"
        } else {
            "with-bindings*"
        },
        &[bindings, function],
    )
}
pub fn var_definitions() -> &'static [NativeDefinition] {
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
        def!("with-bindings*", Arity::AtLeast(2), with_bindings),
        def!("with-redefs-fn", Arity::Exact(2), with_redefs),
        def!("alter-var-root", Arity::AtLeast(2), alter_root),
        def!("alter-meta!", Arity::AtLeast(2), alter_meta),
        def!("reset-meta!", Arity::Exact(2), reset_meta),
        def!("var-set", Arity::Exact(2), |c, a| {
            c.set_var_binding(var(a, 0)?, &a[1])?;
            Ok(a[1].clone())
        }),
        def!("thread-bound?", Arity::AtLeast(0), |c, a| {
            let mut bound = true;
            for i in 0..a.len() {
                bound &= c.var_is_thread_bound(var(a, i)?).map_err(VarError::from)?;
            }
            Ok(c.boolean(bound))
        }),
        def!("binding", Arity::AtLeast(3), |c, a| scope_macro(
            c, a, false, false
        )),
        def!("with-bindings", Arity::AtLeast(3), |c, a| scope_macro(
            c, a, false, true
        )),
        def!("with-redefs", Arity::AtLeast(3), |c, a| scope_macro(
            c, a, true, false
        )),
    ];
    DEFINITIONS
}
