//! Explicit native registry and a checked ABI. Function pointers cannot retain
//! runtime roots, so registering a callback cannot introduce hidden GC cycles.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
}
impl Arity {
    pub fn accepts(self, count: usize) -> bool {
        match self {
            Self::Exact(n) => count == n,
            Self::AtLeast(n) => count >= n,
            Self::Range { min, max } => (min..=max).contains(&count),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeEffects {
    Pure,
    /// Reads or mutates this interpreter context; no external capability.
    Context,
    /// Effects are supplied by embedding code, never discovered from a name.
    Host,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeErrorClass {
    Reader,
    Arity,
    Bounds,
    Type,
    Arithmetic,
    UnsupportedNumber,
    Host,
    WrongOwner,
    Collection,
    Invocation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMetadata {
    pub namespace: &'static str,
    pub name: &'static str,
    pub arity: Arity,
    pub effects: NativeEffects,
    /// Declared language errors, suitable for embedding/permission inspection.
    /// The ABI itself also checks owner identity before and after callbacks.
    pub errors: &'static [NativeErrorClass],
}
impl NativeMetadata {
    pub fn qualified_name(self) -> Name {
        Name::new(Some(self.namespace), self.name)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeError {
    /// Ordinary EOF without an explicit sentinel; malformed source is ReadError.
    EndOfInput,
    StringBounds {
        start: i64,
        end: i64,
        length: usize,
    },
    Arity {
        expected: Arity,
        actual: usize,
    },
    Type {
        argument: usize,
        expected: &'static str,
    },
    Arithmetic(ArithmeticError),
    UnsupportedNumber(&'static str),
    Host(String),
    WrongOwner,
    Collection(CollectionError),
    /// A higher-order native propagates the original failure without losing its
    /// exception identity, phase, source frames, or terminal classification.
    Invocation(ForwardedError),
}

#[derive(Clone, Debug)]
pub struct ForwardedError(Rc<InvokeError>);
impl PartialEq for ForwardedError {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for ForwardedError {}
impl ForwardedError {
    pub fn error(&self) -> &InvokeError {
        &self.0
    }
    pub(super) fn into_error(self) -> InvokeError {
        Rc::try_unwrap(self.0).unwrap_or_else(|error| (*error).clone())
    }
}
impl From<InvokeError> for NativeError {
    fn from(error: InvokeError) -> Self {
        Self::Invocation(ForwardedError(Rc::new(error)))
    }
}
impl From<CollectionError> for NativeError {
    fn from(error: CollectionError) -> Self {
        if error == CollectionError::WrongOwner {
            Self::WrongOwner
        } else {
            Self::Collection(error)
        }
    }
}
impl NativeError {
    pub fn class(&self) -> NativeErrorClass {
        match self {
            Self::Arity { .. } => NativeErrorClass::Arity,
            Self::StringBounds { .. } => NativeErrorClass::Bounds,
            Self::Type { .. } => NativeErrorClass::Type,
            Self::EndOfInput => NativeErrorClass::Reader,
            Self::Arithmetic(_) => NativeErrorClass::Arithmetic,
            Self::UnsupportedNumber(_) => NativeErrorClass::UnsupportedNumber,
            Self::Host(_) => NativeErrorClass::Host,
            Self::WrongOwner => NativeErrorClass::WrongOwner,
            Self::Collection(_) => NativeErrorClass::Collection,
            Self::Invocation(_) => NativeErrorClass::Invocation,
        }
    }
}
impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Reentrant callbacks may wrap an interpreter error as Host. Do
            // not repeatedly debug-escape that message at each call boundary.
            Self::Host(message) => f.write_str(message),
            Self::EndOfInput => f.write_str("EOF while reading"),
            Self::StringBounds { start, end, length } => write!(
                f,
                "substring indices {start}..{end} must satisfy 0 <= start <= end <= UTF-16 length {length}"
            ),
            Self::Invocation(error) => error.error().fmt(f),
            other => write!(f, "{other:?}"),
        }
    }
}
impl std::error::Error for NativeError {}

pub type NativeResult = Result<RootedValue, NativeError>;
pub type NativeCallback = fn(&mut Context, &[RootedValue]) -> NativeResult;
#[derive(Debug)]
pub struct NativeDefinition {
    pub metadata: NativeMetadata,
    pub callback: NativeCallback,
}

impl Context {
    /// Make an owned callable without exposing it under a language name.
    pub fn native_function(&self, definition: &'static NativeDefinition) -> RootedValue {
        self.owner.value(Value::Native(definition))
    }

    /// Install a native root in a stable Var. Previously analyzed reads observe
    /// replacements; a directly retained callable keeps its original callback.
    pub fn register_native(&mut self, definition: &'static NativeDefinition) -> RootedValue {
        let ns = self
            .create_namespace(Name::new(None, definition.metadata.namespace))
            .unwrap();
        let var = self
            .intern_var(
                ns.namespace_id().unwrap(),
                Name::new(None, definition.metadata.name),
            )
            .unwrap();
        let value = self.native_function(definition);
        self.set_var_root(var.var_id().unwrap(), &value).unwrap();
        self.owner
            .store
            .borrow_mut()
            .vars
            .get_mut(var.var_id().unwrap().0)
            .exposed = true;
        if self.is_macro_var(var.var_id().unwrap()) {
            // Explicit function installation replaces a prior macro role;
            // register_macro re-enables that role after installing its root.
            let _ = self.set_var_macro(var.var_id().unwrap(), false);
        }
        value
    }

    /// Invoke through the same ABI used by language calls. Arguments must be
    /// rooted in this context. No storage borrow is held across host code;
    /// callbacks may collect or reenter evaluation. A reset during a callback
    /// is detected and reported, as is returning another context's value.
    pub fn invoke_native(
        &mut self,
        definition: &'static NativeDefinition,
        arguments: &[RootedValue],
    ) -> NativeResult {
        assert_execution_enabled();
        let _scope = self.accounting.enter();
        self.check_native(definition)
            .map_err(InvokeError::Dispatch)?;
        self.poll_execution(PollPoint::Function)
            .map_err(InvokeError::Dispatch)?;
        self.accounting
            .work(arguments.len() as u64)
            .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        if arguments
            .iter()
            .any(|value| !Rc::ptr_eq(&value.owner, &self.owner))
        {
            return Err(NativeError::WrongOwner);
        }
        if !definition.metadata.arity.accepts(arguments.len()) {
            return Err(NativeError::Arity {
                expected: definition.metadata.arity,
                actual: arguments.len(),
            });
        }
        if self.active_calls >= self.accounting.depths().0 {
            return Err(InvokeError::Dispatch(
                self.accounting
                    .kind(self.accounting.fail(ResourceError::CallDepthExceeded)),
            )
            .into());
        }
        let owner = self.owner.clone();
        self.active_calls += 1;
        let result = (definition.callback)(self, arguments);
        self.active_calls -= 1;
        if let Err(error) = self.accounting.check() {
            let forwarded = matches!(&result, Err(NativeError::Invocation(e)) if match e.error() {
                InvokeError::Dispatch(kind) => kind.class() == ErrorClass::Control,
                InvokeError::Execution(error) => error.class() == ErrorClass::Control,
            });
            if !forwarded {
                return Err(InvokeError::Dispatch(self.accounting.kind(error)).into());
            }
        }
        if !Rc::ptr_eq(&owner, &self.owner) {
            return Err(NativeError::WrongOwner);
        }
        let result = result?;
        if !Rc::ptr_eq(&self.owner, &result.owner) {
            return Err(NativeError::WrongOwner);
        }
        Ok(result)
    }
}

#[derive(Clone, Copy)]
enum General {
    Equal,
    NotEqual,
    Identical,
    Identity,
    Not,
    Boolean,
    Nil,
    Some,
    True,
    False,
    BooleanP,
    String,
    Char,
    Symbol,
    Keyword,
    Number,
    Integer,
    Int,
    Float,
    Double,
    Ratio,
    Rational,
    Decimal,
    PosInt,
    NegInt,
    NatInt,
    List,
    Vector,
    Map,
    Set,
    Sequential,
    Seq,
    Coll,
    Associative,
    Counted,
    Fn,
    Ifn,
    SimpleSymbol,
    QualifiedSymbol,
    SimpleKeyword,
    QualifiedKeyword,
}
fn general(context: &mut Context, args: &[RootedValue], operation: General) -> NativeResult {
    use General::*;
    if matches!(operation, Identity) {
        return Ok(args[0].clone());
    }
    if matches!(operation, Equal | NotEqual) {
        let mut equal = true;
        for pair in args.windows(2) {
            if !context.value_eq(&pair[0], &pair[1])? {
                equal = false;
                break;
            }
        }
        return Ok(context.boolean(if matches!(operation, Equal) {
            equal
        } else {
            !equal
        }));
    }
    if matches!(operation, Identical) {
        return Ok(context.boolean(args[0].same_identity(&args[1])));
    }
    let value = args[0].inspect();
    let result = match operation {
        Not => matches!(value, ValueView::Nil | ValueView::Boolean(false)),
        Boolean => !matches!(value, ValueView::Nil | ValueView::Boolean(false)),
        Nil => matches!(value, ValueView::Nil),
        Some => !matches!(value, ValueView::Nil),
        True => matches!(value, ValueView::Boolean(true)),
        False => matches!(value, ValueView::Boolean(false)),
        BooleanP => matches!(value, ValueView::Boolean(_)),
        String => matches!(value, ValueView::String(_)),
        Char => matches!(value, ValueView::Character(_)),
        Symbol => matches!(value, ValueView::Symbol(_)),
        Keyword => matches!(value, ValueView::Keyword(_)),
        Number => matches!(
            value,
            ValueView::Integer(_) | ValueView::Double(_) | ValueView::Ratio { .. }
        ),
        Integer | Int => matches!(value, ValueView::Integer(_)),
        Float | Double => matches!(value, ValueView::Double(_)),
        Ratio => matches!(value, ValueView::Ratio { .. }),
        Rational => matches!(value, ValueView::Integer(_) | ValueView::Ratio { .. }),
        Decimal => false,
        PosInt => matches!(value, ValueView::Integer(n) if n > 0),
        NegInt => matches!(value, ValueView::Integer(n) if n < 0),
        NatInt => matches!(value, ValueView::Integer(n) if n >= 0),
        List => matches!(value, ValueView::List(_)),
        Seq => matches!(
            value,
            ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq
        ),
        Vector => matches!(value, ValueView::Vector(_)),
        Map => matches!(value, ValueView::Map(_) | ValueView::Record(_)),
        Set => matches!(value, ValueView::Set(_)),
        Sequential => matches!(
            value,
            ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq | ValueView::Vector(_)
        ),
        Coll => matches!(
            value,
            ValueView::List(_)
                | ValueView::Seq(_)
                | ValueView::LazySeq
                | ValueView::Vector(_)
                | ValueView::Map(_)
                | ValueView::Record(_)
                | ValueView::Set(_)
        ),
        Associative => matches!(
            value,
            ValueView::Vector(_) | ValueView::Map(_) | ValueView::Record(_)
        ),
        Counted => {
            let store = context.owner.store.borrow();
            matches!(store.heap_value(args[0].value()), Option::Some(HeapValue::List(v)) if v.is_list())
                || matches!(
                    value,
                    ValueView::Vector(_)
                        | ValueView::Map(_)
                        | ValueView::Record(_)
                        | ValueView::Set(_)
                )
                || matches!(store.heap_value(args[0].value()), Option::Some(HeapValue::ArgumentSeq(v)) if v.counted)
        }
        Fn => matches!(value, ValueView::NativeFunction(_) | ValueView::Function),
        Ifn => matches!(
            value,
            ValueView::NativeFunction(_)
                | ValueView::Var(_)
                | ValueView::Function
                | ValueView::Symbol(_)
                | ValueView::Keyword(_)
                | ValueView::Vector(_)
                | ValueView::Map(_)
                | ValueView::Set(_)
        ),
        SimpleSymbol => matches!(value, ValueView::Symbol(ref n) if n.namespace().is_none()),
        QualifiedSymbol => matches!(value, ValueView::Symbol(ref n) if n.namespace().is_some()),
        SimpleKeyword => matches!(value, ValueView::Keyword(ref n) if n.namespace().is_none()),
        QualifiedKeyword => matches!(value, ValueView::Keyword(ref n) if n.namespace().is_some()),
        Equal | NotEqual | Identical | Identity => unreachable!(),
    };
    Ok(context.boolean(result))
}

// A finite, inspectable registry. No string-to-Rust/OS lookup path exists.
macro_rules! definition {
    ($name:literal, $arity:expr, general $op:ident) => { definition!($name, $arity, general $op, Pure) };
    ($name:literal, $arity:expr, general $op:ident, $effect:ident) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::$effect,
                errors: &[NativeErrorClass::Arity, NativeErrorClass::Invocation],
            },
            callback: |context, args| general(context, args, General::$op),
        }
    };
    ($name:literal, $arity:expr, numeric $op:ident) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::Pure,
                errors: &[
                    NativeErrorClass::Arity,
                    NativeErrorClass::Type,
                    NativeErrorClass::Arithmetic,
                ],
            },
            callback: |context, args| numbers::call(context, args, numbers::Operation::$op),
        }
    };
    ($name:literal, unsupported) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: Arity::Exact(1),
                effects: NativeEffects::Pure,
                errors: &[NativeErrorClass::Arity, NativeErrorClass::UnsupportedNumber],
            },
            callback: |_, _| Err(NativeError::UnsupportedNumber($name)),
        }
    };
}

pub fn builtin_definitions() -> &'static [NativeDefinition] {
    use Arity::{AtLeast, Exact};
    static DEFINITIONS: &[NativeDefinition] = &[
        definition!("+", AtLeast(0), numeric Add),
        definition!("-", AtLeast(1), numeric Subtract),
        definition!("*", AtLeast(0), numeric Multiply),
        definition!("/", AtLeast(1), numeric Divide),
        definition!("inc", Exact(1), numeric Inc),
        definition!("dec", Exact(1), numeric Dec),
        definition!("quot", Exact(2), numeric Quot),
        definition!("rem", Exact(2), numeric Rem),
        definition!("mod", Exact(2), numeric Mod),
        definition!("==", AtLeast(1), numeric Equal),
        definition!("<", AtLeast(1), numeric Less),
        definition!("<=", AtLeast(1), numeric LessEqual),
        definition!(">", AtLeast(1), numeric Greater),
        definition!(">=", AtLeast(1), numeric GreaterEqual),
        definition!("zero?", Exact(1), numeric Zero),
        definition!("pos?", Exact(1), numeric Positive),
        definition!("neg?", Exact(1), numeric Negative),
        definition!("even?", Exact(1), numeric Even),
        definition!("odd?", Exact(1), numeric Odd),
        definition!("min", AtLeast(1), numeric Min),
        definition!("max", AtLeast(1), numeric Max),
        definition!("byte", Exact(1), numeric Byte),
        definition!("short", Exact(1), numeric Short),
        definition!("int", Exact(1), numeric Int),
        definition!("long", Exact(1), numeric Long),
        definition!("float", Exact(1), numeric Float),
        definition!("double", Exact(1), numeric Double),
        definition!("char", Exact(1), numeric Char),
        definition!("numerator", Exact(1), numeric Numerator),
        definition!("denominator", Exact(1), numeric Denominator),
        definition!("bigint", unsupported),
        definition!("biginteger", unsupported),
        definition!("bigdec", unsupported),
        definition!("=", AtLeast(1), general Equal, Context),
        definition!("not=", AtLeast(1), general NotEqual, Context),
        definition!("identical?", Exact(2), general Identical),
        definition!("identity", Exact(1), general Identity),
        definition!("not", Exact(1), general Not),
        definition!("boolean", Exact(1), general Boolean),
        definition!("nil?", Exact(1), general Nil),
        definition!("some?", Exact(1), general Some),
        definition!("true?", Exact(1), general True),
        definition!("false?", Exact(1), general False),
        definition!("boolean?", Exact(1), general BooleanP),
        definition!("string?", Exact(1), general String),
        definition!("char?", Exact(1), general Char),
        definition!("symbol?", Exact(1), general Symbol),
        definition!("keyword?", Exact(1), general Keyword),
        definition!("number?", Exact(1), general Number),
        definition!("integer?", Exact(1), general Integer),
        definition!("int?", Exact(1), general Int),
        definition!("float?", Exact(1), general Float),
        definition!("double?", Exact(1), general Double),
        definition!("ratio?", Exact(1), general Ratio),
        definition!("rational?", Exact(1), general Rational),
        definition!("decimal?", Exact(1), general Decimal),
        definition!("pos-int?", Exact(1), general PosInt),
        definition!("neg-int?", Exact(1), general NegInt),
        definition!("nat-int?", Exact(1), general NatInt),
        definition!("list?", Exact(1), general List),
        definition!("vector?", Exact(1), general Vector),
        definition!("map?", Exact(1), general Map),
        definition!("set?", Exact(1), general Set),
        definition!("sequential?", Exact(1), general Sequential),
        definition!("seq?", Exact(1), general Seq),
        definition!("coll?", Exact(1), general Coll),
        definition!("associative?", Exact(1), general Associative),
        definition!("counted?", Exact(1), general Counted),
        definition!("fn?", Exact(1), general Fn),
        definition!("ifn?", Exact(1), general Ifn),
        definition!("simple-symbol?", Exact(1), general SimpleSymbol),
        definition!("qualified-symbol?", Exact(1), general QualifiedSymbol),
        definition!("simple-keyword?", Exact(1), general SimpleKeyword),
        definition!("qualified-keyword?", Exact(1), general QualifiedKeyword),
    ];
    DEFINITIONS
}
