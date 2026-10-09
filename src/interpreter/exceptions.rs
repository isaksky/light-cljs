//! Portable exception objects. These are traced language data, not Java classes.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExceptionCategory {
    Throwable,
    Exception,
    ExceptionInfo,
    AssertionError,
}
impl ExceptionCategory {
    pub(super) fn catches(self, thrown: Self) -> bool {
        self == Self::Throwable
            || self == thrown
            || (self == Self::Exception && thrown == Self::ExceptionInfo)
    }
    pub(super) fn named(name: &Name) -> Option<Self> {
        if name.namespace().is_some() {
            return None;
        }
        match String::from_utf16(name.name()).ok()?.as_str() {
            "AssertionError" | "java.lang.AssertionError" => Some(Self::AssertionError),
            "Throwable" | "java.lang.Throwable" => Some(Self::Throwable),
            "Exception" | "java.lang.Exception" => Some(Self::Exception),
            "ExceptionInfo" | "clojure.lang.ExceptionInfo" => Some(Self::ExceptionInfo),
            _ => None,
        }
    }
}

pub(super) struct Exception {
    pub(super) category: ExceptionCategory,
    // Portable Exception constructors are checked; synthesized runtime failures
    // have the same public catch category but must pass validators unchanged.
    pub(super) checked: bool,
    pub(super) message: Value,
    pub(super) data: Value,
    pub(super) cause: Value,
}
impl Context {
    /// Construct a portable exception. Message is string/nil, data is a map for
    /// ExceptionInfo (nil otherwise), and cause is an exception/nil in this owner.
    pub fn exception(
        &self,
        category: ExceptionCategory,
        message: &RootedValue,
        data: &RootedValue,
        cause: &RootedValue,
    ) -> NativeResult {
        self.charge_allocation(1)?;
        if [message, data, cause]
            .iter()
            .any(|v| !Rc::ptr_eq(&v.owner, &self.owner))
        {
            return Err(NativeError::WrongOwner);
        }
        if !matches!(message.inspect(), ValueView::String(_) | ValueView::Nil) {
            return Err(NativeError::Type {
                argument: 0,
                expected: "string or nil",
            });
        }
        if !(matches!(data.inspect(), ValueView::Map(_))
            && category == ExceptionCategory::ExceptionInfo
            || matches!(data.inspect(), ValueView::Nil)
                && category != ExceptionCategory::ExceptionInfo)
        {
            return Err(NativeError::Type {
                argument: 1,
                expected: "exception data map",
            });
        }
        if !matches!(cause.inspect(), ValueView::Exception(_) | ValueView::Nil) {
            return Err(NativeError::Type {
                argument: 2,
                expected: "exception or nil",
            });
        }
        let id = HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Exception(Exception {
                    category,
                    checked: category == ExceptionCategory::Exception,
                    message: message.value(),
                    data: data.value(),
                    cause: cause.value(),
                })),
        );
        Ok(self.owner.checked_value(Value::Heap(id))?)
    }
}
impl Context {
    pub(super) fn runtime_exception(
        &self,
        category: ExceptionCategory,
        message: &RootedValue,
        data: &RootedValue,
        cause: &RootedValue,
    ) -> NativeResult {
        let value = self.exception(category, message, data, cause)?;
        let mut store = self.owner.store.borrow_mut();
        let HeapValue::Exception(exception) = store.heap.get_mut(value.heap_id().unwrap().0) else {
            unreachable!()
        };
        exception.checked = false;
        Ok(value)
    }
}
impl RootedValue {
    pub fn exception_message(&self) -> Option<RootedValue> {
        self.exception_field(|e| e.message)
    }
    pub fn exception_data(&self) -> Option<RootedValue> {
        self.exception_field(|e| e.data)
    }
    pub fn exception_cause(&self) -> Option<RootedValue> {
        self.exception_field(|e| e.cause)
    }
    fn exception_field(&self, field: fn(&Exception) -> Value) -> Option<RootedValue> {
        let store = self.owner.store.borrow();
        let HeapValue::Exception(e) = store.heap_value(self.value())? else {
            return None;
        };
        Some(self.owner.value(field(e)))
    }
}
fn info(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let data = if matches!(args[1].inspect(), ValueView::Nil) {
        c.map(&[])?
    } else {
        args[1].clone()
    };
    c.exception(
        ExceptionCategory::ExceptionInfo,
        &args[0],
        &data,
        args.get(2).unwrap_or(&c.nil()),
    )
}
fn construct(c: &mut Context, args: &[RootedValue], category: ExceptionCategory) -> NativeResult {
    c.exception(
        category,
        args.first().unwrap_or(&c.nil()),
        &c.nil(),
        args.get(1).unwrap_or(&c.nil()),
    )
}
fn message(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    Ok(args[0].exception_message().unwrap_or_else(|| c.nil()))
}
fn data(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    Ok(args[0].exception_data().unwrap_or_else(|| c.nil()))
}
fn cause(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    Ok(args[0].exception_cause().unwrap_or_else(|| c.nil()))
}
macro_rules! definition {
    ($name:literal, $arity:expr, $callback:expr) => {
        NativeDefinition {
            metadata: NativeMetadata {
                namespace: "clojure.core",
                name: $name,
                arity: $arity,
                effects: NativeEffects::Pure,
                errors: &[NativeErrorClass::Arity, NativeErrorClass::Type],
            },
            callback: $callback,
        }
    };
}
pub fn exception_definitions() -> &'static [NativeDefinition] {
    use Arity::{Exact, Range};
    static DEFINITIONS: &[NativeDefinition] = &[
        definition!("AssertionError.", Range { min: 0, max: 1 }, |c, a| {
            construct(c, a, ExceptionCategory::AssertionError)
        }),
        definition!(
            "java.lang.AssertionError.",
            Range { min: 0, max: 1 },
            |c, a| construct(c, a, ExceptionCategory::AssertionError)
        ),
        definition!("ex-info", Range { min: 2, max: 3 }, info),
        definition!("ex-message", Exact(1), message),
        definition!("ex-data", Exact(1), data),
        definition!("ex-cause", Exact(1), cause),
        definition!("Exception.", Range { min: 0, max: 2 }, |c, a| construct(
            c,
            a,
            ExceptionCategory::Exception
        )),
        definition!("Throwable.", Range { min: 0, max: 2 }, |c, a| construct(
            c,
            a,
            ExceptionCategory::Throwable
        )),
        definition!("java.lang.Exception.", Range { min: 0, max: 2 }, |c, a| {
            construct(c, a, ExceptionCategory::Exception)
        }),
        definition!("java.lang.Throwable.", Range { min: 0, max: 2 }, |c, a| {
            construct(c, a, ExceptionCategory::Throwable)
        }),
    ];
    DEFINITIONS
}
