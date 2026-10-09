//! Effectful macro calls use the ordinary checked invocation ABI. Expansion data
//! is lowered directly into owned forms, retaining the original call location.
use super::*;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct MacroExpansionStats {
    pub name: Name,
    /// Originating fn/letfn form; None for native or composed host callables.
    pub definition: Option<DiagnosticLocation>,
    pub expansions: u64,
    /// Semantic form nodes, including metadata, produced for analysis.
    pub generated_forms: u64,
    pub elapsed: Duration,
}
#[derive(Clone, Copy)]
pub(super) struct BindingIdentity(pub(super) u64);
#[derive(Default)]
pub(super) struct MacroState {
    enabled: bool,
    pub(super) analysis_expansion: bool,
    next_binding: u64,
    pub(super) invocations: u64,
    pub(super) form_depth: usize,
    pub(super) stats: Vec<MacroExpansionStats>,
    pub(super) code_diagnostics: bool,
    pub(super) code_stats: Vec<CodeGenerationStats>,
    environments: Vec<RootedValue>,
    pub(super) sources: Vec<Form>,
}

impl MacroState {
    pub(super) fn binding_identity(&mut self) -> BindingIdentity {
        let id = BindingIdentity(self.next_binding);
        self.next_binding = self
            .next_binding
            .checked_add(1)
            .expect("lexical identities exhausted");
        id
    }
}

pub(super) fn primitive(name: &Name) -> bool {
    name.namespace().is_none()
        && [
            "def", "var", "quote", "if", "do", "let*", "loop*", "recur", "letfn*", "throw", "try",
            "set!", "case*",
        ]
        .iter()
        .any(|s| name == &Name::new(None, s))
}
impl Context {
    /// Invoke one explicitly resolved macro with an embedding-defined environment.
    /// Unlike SCI `macroexpand`, this does not interpret the shape of `env` or
    /// resolve its operator. Compiler clients can supply a CLJS analysis env.
    /// Arguments and output remain owned data, with call-site provenance.
    pub fn expand_macro_call(
        &mut self,
        form: &Form,
        var: &RootedValue,
        env: &RootedValue,
    ) -> Result<Form, EvalError> {
        let _scope = self.accounting.enter();
        let name = match var.inspect() {
            ValueView::Var(name) => name,
            _ => {
                return Err(evaluation::analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("macro expansion requires a macro Var"),
                ));
            }
        };
        if !Rc::ptr_eq(&self.owner, &var.owner)
            || !Rc::ptr_eq(&self.owner, &form.data().owner)
            || !Rc::ptr_eq(&self.owner, &env.owner)
        {
            return Err(evaluation::analysis_error(form, EvalErrorKind::WrongOwner));
        }
        let id = var.var_id().unwrap();
        self.check_var_access(id)
            .map_err(|kind| evaluation::analysis_error(form, kind))?;
        if !self.is_macro_var(id) {
            return Err(evaluation::analysis_error(
                form,
                EvalErrorKind::InvalidForm("macro expansion requires a macro Var"),
            ));
        }
        let result = (|| {
            let mut metadata = form.data().metadata().unwrap_or(self.map(&[])?);
            let (line, column) = form
                .span()
                .source()
                .line_column(form.span().bytes().start)
                .unwrap();
            for (key, value) in [("line", line), ("column", column)] {
                metadata = self.assoc(
                    &metadata,
                    &self.keyword(Name::new(None, key)),
                    &self.integer(value as i64),
                )?;
            }
            let data = self.with_meta(&form.data(), &metadata)?;
            self.macros.sources.push(form.clone());
            let result = self.invoke_macro(&data, env, id, &name, true);
            self.macros.sources.pop();
            let (value, index) = result?;
            let started = self.macros.enabled.then(Instant::now);
            self.realize_key(&value).map_err(deferred::invoke_error)?;
            let expanded = self
                .macro_form(&value, form, name.clone())
                .map_err(|e| InvokeError::Execution(Box::new(e.into())))?;
            if let Some(index) = index {
                self.macros.stats[index].generated_forms += expanded.nodes().len() as u64;
                if let Some(started) = started {
                    self.macros.stats[index].elapsed += started.elapsed();
                }
            }
            self.accounting
                .check()
                .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
            Ok(expanded)
        })();
        result.map_err(|error| self.expansion_error(form, name, error))
    }

    /// Enable optional attribution. Existing counters survive disabling; reset
    /// drops them. No clock reads or report allocation occur while disabled.
    pub fn set_macro_diagnostics(&mut self, enabled: bool) {
        self.macros.enabled = enabled;
    }
    pub fn macro_expansion_stats(&self) -> &[MacroExpansionStats] {
        &self.macros.stats
    }

    /// Trusted embedding macro registration; language metadata mutation is checked.
    pub fn set_var_macro(&mut self, id: VarId, enabled: bool) -> Result<(), CollectionError> {
        let id = self
            .context_var(id)
            .map_err(|_| CollectionError::WrongOwner)?;
        let var = self
            .var_value(id)
            .map_err(|_| CollectionError::WrongOwner)?;
        let meta = var.metadata().unwrap_or(self.map(&[])?);
        let meta = self.assoc(
            &meta,
            &self.keyword(Name::new(None, "macro")),
            &self.boolean(enabled),
        )?;
        self.set_var_metadata(id, &meta);
        Ok(())
    }
    /// Arity includes the implicit &form and &env parameters. The returned Var
    /// is stable; its root is an ordinary native function callable explicitly
    /// through var/deref, as in SCI. Bare macro symbols are rejected by analysis.
    pub fn register_macro(&mut self, definition: &'static NativeDefinition) -> RootedValue {
        self.register_native(definition);
        let ns = self
            .find_namespace(&Name::new(None, definition.metadata.namespace))
            .unwrap();
        let var = self
            .intern_var(
                ns.namespace_id().unwrap(),
                Name::new(None, definition.metadata.name),
            )
            .unwrap();
        // Resource failures are latched by the common scope and reported at its boundary.
        let _ = self.set_var_macro(var.var_id().unwrap(), true);
        var
    }
    pub(super) fn is_macro_var(&self, id: VarId) -> bool {
        let meta = self.var_value(id).expect("live Var").metadata();
        meta.and_then(|m| {
            m.get(&self.keyword(Name::new(None, "macro")))
                .ok()
                .flatten()
        })
        .is_some_and(|v| !matches!(v.inspect(), ValueView::Nil | ValueView::Boolean(false)))
    }
    pub(super) fn macro_candidate(
        &self,
        data: &RootedValue,
        env: &RootedValue,
    ) -> Result<Option<(VarId, Name)>, EvalErrorKind> {
        if !matches!(
            data.inspect(),
            ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq
        ) {
            return Ok(None);
        }
        let raw = self
            .owner
            .store
            .borrow()
            .sequence_nth(data.value(), 0)
            .map_err(EvalErrorKind::Collection)?;
        let Some(op) = raw
            .map(|v| self.owner.value(v))
            .filter(|v| matches!(v.inspect(), ValueView::Symbol(_)))
        else {
            return Ok(None);
        };
        let ValueView::Symbol(name) = op.inspect() else {
            unreachable!()
        };
        if primitive(&name)
            || (!matches!(env.inspect(), ValueView::Nil)
                && env.get(&op).map_err(EvalErrorKind::Collection)?.is_some())
        {
            return Ok(None);
        }
        let var = self.resolve_var(&name).map_err(EvalErrorKind::Namespace)?;
        Ok(var.and_then(|v| {
            let id = v.var_id().unwrap();
            self.is_macro_var(id)
                .then(|| (id, self.owner.store.borrow().var_name(id)))
        }))
    }
    fn invoke_macro(
        &mut self,
        form: &RootedValue,
        env: &RootedValue,
        var: VarId,
        name: &Name,
        analysis_expansion: bool,
    ) -> Result<(RootedValue, Option<usize>), InvokeError> {
        self.macros.invocations = self
            .macros
            .invocations
            .checked_add(1)
            .expect("macro invocation count exhausted");
        self.poll_execution(PollPoint::Macroexpansion)
            .map_err(InvokeError::Dispatch)?;
        if self.macros.environments.len() >= self.accounting.depths().1 {
            return Err(InvokeError::Dispatch(self.accounting.kind(
                self.accounting.fail(ResourceError::EvaluationDepthExceeded),
            )));
        }
        let raw = form.elements()?;
        self.charge_allocation(raw.len().saturating_add(1) as u64)
            .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        let mut arguments = Vec::with_capacity(raw.len() + 1);
        arguments.push(form.clone());
        arguments.push(env.clone());
        arguments.extend(raw.into_iter().skip(1));
        let root = self.var_get(var).expect("live macro Var");
        let definition = self
            .macros
            .enabled
            .then(|| self.callable_definition(&root))
            .flatten();
        let started = self.macros.enabled.then(Instant::now);
        self.macros.environments.push(env.clone());
        let previous = std::mem::replace(&mut self.macros.analysis_expansion, analysis_expansion);
        let result = self.invoke(&root, &arguments);
        self.macros.analysis_expansion = previous;
        self.macros.environments.pop();
        let index = if let Some(started) = started {
            let index = self
                .macros
                .stats
                .iter()
                .position(|s| &s.name == name && s.definition == definition)
                .unwrap_or_else(|| {
                    self.macros.stats.push(MacroExpansionStats {
                        name: name.clone(),
                        definition,
                        expansions: 0,
                        generated_forms: 0,
                        elapsed: Duration::ZERO,
                    });
                    self.macros.stats.len() - 1
                });
            let stats = &mut self.macros.stats[index];
            stats.expansions += 1;
            stats.elapsed += started.elapsed();
            Some(index)
        } else {
            None
        };
        result.map(|value| (value, index))
    }
    fn expansion_error(&self, form: &Form, name: Name, error: InvokeError) -> EvalError {
        let mut failure = match error {
            InvokeError::Dispatch(kind) => evaluation::analysis_error(form, kind),
            InvokeError::Execution(error) => *error,
        };
        // Preserve body provenance as a frame; the diagnostic itself identifies
        // the source invocation, including callback arity/host/control failures.
        if failure.span.source().id() != form.span().source().id()
            || failure.span.bytes() != form.span().bytes()
        {
            failure.frames.insert(
                0,
                SourceFrame {
                    span: (*failure.span).clone(),
                    origin: failure.origin.clone(),
                },
            );
        }
        failure.span = Box::new(form.span());
        failure.phase = EvalPhase::Macroexpansion;
        failure.origin = Origin::MacroExpansion(Rc::new(name));
        failure
    }
    pub(super) fn expand_form(
        &mut self,
        form: &Form,
        env: &RootedValue,
    ) -> Result<Form, EvalError> {
        let mut form = form.clone();
        let mut count = 0;
        loop {
            let Some((var, name)) = self
                .macro_candidate(&form.data(), env)
                .map_err(|e| evaluation::analysis_error(&form, e))?
            else {
                return Ok(form);
            };
            if count >= self.accounting.depths().1 {
                let error = self
                    .accounting
                    .kind(self.accounting.fail(ResourceError::EvaluationDepthExceeded));
                return Err(self.expansion_error(&form, name, InvokeError::Dispatch(error)));
            }
            count += 1;
            let (result, diagnostic_index) = (|| {
                let mut metadata = form.data().metadata().unwrap_or(self.map(&[])?);
                let (line, column) = form
                    .span()
                    .source()
                    .line_column(form.span().bytes().start)
                    .unwrap();
                for (key, value) in [("line", line), ("column", column)] {
                    metadata = self.assoc(
                        &metadata,
                        &self.keyword(Name::new(None, key)),
                        &self.integer(value as i64),
                    )?;
                }
                let data = self.with_meta(&form.data(), &metadata)?;
                let env = if matches!(env.inspect(), ValueView::Nil) {
                    self.map(&[])?
                } else {
                    env.clone()
                };
                self.macros.sources.push(form.clone());
                let result = self.invoke_macro(&data, &env, var, &name, true);
                self.macros.sources.pop();
                result
            })()
            .map_err(|e| self.expansion_error(&form, name.clone(), e))?;
            let started = self.macros.enabled.then(Instant::now);
            self.realize_key(&result).map_err(|e| {
                self.expansion_error(&form, name.clone(), deferred::invoke_error(e))
            })?;
            let result = if matches!(
                result.inspect(),
                ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq
            ) {
                (|| -> Result<RootedValue, CollectionError> {
                    let mut metadata = form.data().metadata().unwrap_or(self.map(&[])?);
                    if let Some(generated) = result.metadata() {
                        for (key, value) in generated.entries()? {
                            metadata = self.assoc(&metadata, &key, &value)?;
                        }
                    }
                    self.with_meta(&result, &metadata)
                })()
                .map_err(|e| self.expansion_error(&form, name.clone(), InvokeError::from(e)))?
            } else {
                result
            };
            let next = self.macro_form(&result, &form, name.clone()).map_err(|e| {
                self.expansion_error(
                    &form,
                    name.clone(),
                    InvokeError::Execution(Box::new(e.into())),
                )
            })?;
            if let Some(index) = diagnostic_index
                && let Some(started) = started
                && let Some(stats) = self.macros.stats.get_mut(index)
            {
                stats.generated_forms += next.nodes().len() as u64;
                stats.elapsed += started.elapsed();
            }
            form = next;
        }
    }
    /// Expand one outer call. The env-first language overload accepts a map or
    /// nil; map keys suppress macro resolution, including keys mapped to nil.
    pub fn macroexpand_1(
        &mut self,
        form: &RootedValue,
        env: Option<&RootedValue>,
    ) -> Result<RootedValue, InvokeError> {
        self.expand_data(form, env, false)
    }
    pub fn macroexpand(
        &mut self,
        form: &RootedValue,
        env: Option<&RootedValue>,
    ) -> Result<RootedValue, InvokeError> {
        self.expand_data(form, env, true)
    }
    fn expand_data(
        &mut self,
        form: &RootedValue,
        supplied: Option<&RootedValue>,
        repeat: bool,
    ) -> Result<RootedValue, InvokeError> {
        let _scope = self.accounting.enter();
        let result = self.expand_data_inner(form, supplied, repeat);
        if !result
            .as_ref()
            .is_err_and(|e| e.class() == ErrorClass::Control)
        {
            self.accounting
                .check()
                .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        }
        result
    }
    fn expand_data_inner(
        &mut self,
        form: &RootedValue,
        supplied: Option<&RootedValue>,
        repeat: bool,
    ) -> Result<RootedValue, InvokeError> {
        if !Rc::ptr_eq(&self.owner, &form.owner)
            || supplied.is_some_and(|e| !Rc::ptr_eq(&self.owner, &e.owner))
        {
            return Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner));
        }
        self.poll_execution(PollPoint::Macroexpansion)
            .map_err(InvokeError::Dispatch)?;
        let mut env = self
            .macros
            .environments
            .last()
            .cloned()
            .unwrap_or(self.map(&[])?);
        if let Some(supplied) = supplied
            && !matches!(supplied.inspect(), ValueView::Nil)
        {
            if matches!(env.inspect(), ValueView::Nil) {
                env = self.map(&[])?;
            }
            for (k, v) in supplied.entries()? {
                env = self.assoc(&env, &k, &v)?;
            }
        }
        let mut form = form.clone();
        let mut count = 0;
        loop {
            if matches!(form.inspect(), ValueView::LazySeq) {
                self.seq(&form).map_err(deferred::invoke_error)?;
            }
            let Some((var, name)) = self
                .macro_candidate(&form, &env)
                .map_err(InvokeError::Dispatch)?
            else {
                return Ok(form);
            };
            if count >= self.accounting.depths().1 {
                return Err(InvokeError::Dispatch(self.accounting.kind(
                    self.accounting.fail(ResourceError::EvaluationDepthExceeded),
                )));
            }
            count += 1;
            let (expanded, _) = self.invoke_macro(&form, &env, var, &name, false)?;
            if !repeat || expanded.same_identity(&form) {
                return Ok(expanded);
            }
            form = expanded;
        }
    }
}

fn defmacro(context: &mut Context, args: &[RootedValue]) -> NativeResult {
    let name = &args[2];
    if !matches!(name.inspect(), ValueView::Symbol(ref n) if n.namespace().is_none()) {
        return Err(NativeError::Type {
            argument: 2,
            expected: "unqualified macro symbol",
        });
    }
    let mut metadata = name.metadata().unwrap_or(context.map(&[])?);
    let mut start = 3;
    if matches!(
        args.get(start).map(RootedValue::inspect),
        Some(ValueView::String(_))
    ) {
        metadata = context.assoc(
            &metadata,
            &context.keyword(Name::new(None, "doc")),
            &args[start],
        )?;
        start += 1;
    }
    if matches!(
        args.get(start).map(RootedValue::inspect),
        Some(ValueView::Map(_))
    ) {
        for (k, v) in args[start].entries()? {
            metadata = context.assoc(&metadata, &k, &v)?;
        }
        start += 1;
    }
    let mut body = &args[start..];
    let multi = matches!(
        body.first().map(RootedValue::inspect),
        Some(ValueView::List(_) | ValueView::Seq(_))
    );
    if multi
        && let Some(last) = body.last()
        && matches!(last.inspect(), ValueView::Map(_))
    {
        for (k, v) in last.entries()? {
            metadata = context.assoc(&metadata, &k, &v)?;
        }
        body = &body[..body.len() - 1];
    }
    let mut methods = if multi {
        body.iter()
            .map(RootedValue::elements)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![body.to_vec()]
    };
    let mut arglists = Vec::new();
    let mut function = vec![context.symbol(Name::new(Some("clojure.core"), "fn"))];
    for method in &mut methods {
        if !matches!(
            method.first().map(RootedValue::inspect),
            Some(ValueView::Vector(_))
        ) {
            return Err(NativeError::Type {
                argument: start,
                expected: "macro parameter vector",
            });
        }
        arglists.push(method[0].clone());
        let mut params = vec![
            context.symbol(Name::new(None, "&form")),
            context.symbol(Name::new(None, "&env")),
        ];
        params.extend(method[0].elements()?);
        method[0] = context.vector(&params)?;
        if multi {
            function.push(context.list(method)?);
        } else {
            function.extend(method.iter().cloned());
        }
    }
    let quote = context.symbol(Name::new(None, "quote"));
    let arglists = context.list(&[quote, context.eager_sequence(&arglists, true)?])?;
    metadata = context.assoc(
        &metadata,
        &context.keyword(Name::new(None, "arglists")),
        &arglists,
    )?;
    metadata = context.assoc(
        &metadata,
        &context.keyword(Name::new(None, "macro")),
        &context.boolean(true),
    )?;
    let name = context.with_meta(name, &metadata)?;
    Ok(context.list(&[
        context.symbol(Name::new(None, "def")),
        name,
        context.list(&function)?,
    ])?)
}

pub(super) fn core_macro(name: &Name) -> bool {
    [
        "defmacro",
        "ns",
        "refer-clojure",
        "let",
        "loop",
        "fn",
        "defn",
        "defn-",
        "letfn",
        "assert",
        "and",
        "or",
        "if-not",
        "when",
        "when-not",
        "if-let",
        "when-let",
        "if-some",
        "when-some",
        "when-first",
        "cond",
        "condp",
        "->",
        "->>",
        "as->",
        "cond->",
        "cond->>",
        "some->",
        "some->>",
        "comment",
        "case",
        "delay",
        "lazy-seq",
        "for",
        "doseq",
        "dotimes",
        "while",
        "binding",
        "with-bindings",
        "with-redefs",
        "with-in-str",
        "with-out-str",
        "vswap!",
    ]
    .iter()
    .any(|n| name == &Name::new(None, n))
}

pub fn macro_definitions() -> &'static [NativeDefinition] {
    macro_rules! binding {
        ($name:literal, $arity:expr, $callback:ident) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: $arity,
                    effects: NativeEffects::Context,
                    errors: &[
                        NativeErrorClass::Arity,
                        NativeErrorClass::Collection,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: binding_macros::$callback,
            }
        };
    }
    macro_rules! control {
        ($name:literal, $arity:expr, $callback:ident) => {
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
                callback: control_macros::$callback,
            }
        };
    }
    macro_rules! iteration {
        ($name:literal, $arity:expr, $callback:ident) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: $arity,
                    effects: NativeEffects::Context,
                    errors: &[
                        NativeErrorClass::Arity,
                        NativeErrorClass::Collection,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: iteration_macros::$callback,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        iteration!("for", Arity::Exact(4), for_macro),
        iteration!("doseq", Arity::AtLeast(3), doseq),
        iteration!("dotimes", Arity::AtLeast(3), dotimes),
        iteration!("while", Arity::AtLeast(3), while_macro),
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "delay",
                arity: Arity::AtLeast(2),
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: deferred::delay_macro,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "lazy-seq",
                arity: Arity::AtLeast(2),
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: deferred::lazy_macro,
        },
        binding!("let", Arity::AtLeast(3), let_macro),
        binding!("loop", Arity::AtLeast(3), loop_macro),
        binding!("fn", Arity::AtLeast(2), fn_macro),
        binding!("defn", Arity::AtLeast(3), defn_macro),
        binding!("defn-", Arity::AtLeast(3), private_defn),
        binding!("letfn", Arity::AtLeast(3), letfn_macro),
        control!("and", Arity::AtLeast(2), and),
        control!("or", Arity::AtLeast(2), or),
        control!("if-not", Arity::Range { min: 4, max: 5 }, if_not),
        control!("when", Arity::AtLeast(3), when),
        control!("when-not", Arity::AtLeast(3), when_not),
        control!("if-let", Arity::AtLeast(4), if_let),
        control!("if-some", Arity::AtLeast(4), if_some),
        control!("when-let", Arity::AtLeast(3), when_let),
        control!("when-some", Arity::AtLeast(3), when_some),
        control!("when-first", Arity::AtLeast(3), when_first),
        control!("cond", Arity::AtLeast(2), cond),
        control!("condp", Arity::AtLeast(4), condp),
        control!("->", Arity::AtLeast(3), thread_first),
        control!("->>", Arity::AtLeast(3), thread_last),
        control!("as->", Arity::AtLeast(4), as_thread),
        control!("cond->", Arity::AtLeast(3), cond_thread_first),
        control!("cond->>", Arity::AtLeast(3), cond_thread_last),
        control!("some->", Arity::AtLeast(3), some_thread_first),
        control!("some->>", Arity::AtLeast(3), some_thread_last),
        control!("comment", Arity::AtLeast(2), comment),
        control!("assert", Arity::Range { min: 3, max: 4 }, assert),
        control!("case", Arity::AtLeast(3), case),
        binding!("seq-to-map-for-destructuring", Arity::Exact(1), seq_to_map),
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "defmacro",
                arity: Arity::AtLeast(4),
                effects: NativeEffects::Context,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Type,
                    NativeErrorClass::Collection,
                ],
            },
            callback: defmacro,
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "macroexpand-1",
                arity: Arity::Range { min: 1, max: 2 },
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: |c, a| {
                c.macroexpand_1(a.last().unwrap(), (a.len() == 2).then(|| &a[0]))
                    .map_err(Into::into)
            },
        },
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: "macroexpand",
                arity: Arity::Range { min: 1, max: 2 },
                effects: NativeEffects::Context,
                errors: &[NativeErrorClass::Invocation],
            },
            callback: |c, a| {
                c.macroexpand(a.last().unwrap(), (a.len() == 2).then(|| &a[0]))
                    .map_err(Into::into)
            },
        },
    ];
    DEFINITIONS
}
