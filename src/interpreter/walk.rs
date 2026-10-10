//! Eager data walks. User callbacks use the ordinary invocation boundary, and
//! each visited node/sequence step shares the enclosing resource budget.
use super::*;
use collection_core::Cursor;

#[derive(Clone, Copy)]
enum Shape {
    List,
    Seq,
    Vector,
    Entry,
    Map,
    Record,
    Set,
}
struct Traversal {
    shape: Shape,
    cursor: Cursor,
    values: Vec<RootedValue>,
    result: Option<RootedValue>,
    metadata: Option<RootedValue>,
}
impl Traversal {
    fn new(c: &mut Context, form: &RootedValue) -> Result<Option<Self>, NativeError> {
        let shape = match form.inspect() {
            ValueView::List(_) => Shape::List,
            ValueView::Seq(_) | ValueView::LazySeq => Shape::Seq,
            ValueView::Vector(_) if c.is_map_entry(form) => Shape::Entry,
            ValueView::Vector(_) => Shape::Vector,
            ValueView::Map(_) => Shape::Map,
            ValueView::Record(_) => Shape::Record,
            ValueView::Set(_) => Shape::Set,
            _ => return Ok(None),
        };
        c.charge_allocation(1)?;
        Ok(Some(Self {
            shape,
            cursor: Cursor::new(c, form, 0)?,
            values: Vec::new(),
            result: match shape {
                Shape::Map => Some(c.map(&[])?),
                Shape::Record => Some(form.clone()),
                Shape::Set => Some(c.set(&[])?),
                _ => None,
            },
            metadata: if matches!(shape, Shape::Entry) {
                None
            } else {
                form.metadata()
            },
        }))
    }
    fn push(&mut self, c: &mut Context, value: RootedValue) -> Result<(), NativeError> {
        if let Some(result) = &self.result {
            // Stop at a malformed map entry or failed key before invoking the
            // next callback, preserving the effects of an ordinary into.
            self.result = Some(collection_core::conj(c, result, &value)?);
        } else {
            c.charge_allocation(1)?;
            self.values.push(value);
        }
        Ok(())
    }
    fn finish(self, c: &Context) -> NativeResult {
        let result = match self.shape {
            Shape::List => c.list(&self.values)?,
            Shape::Seq => c.eager_sequence_value(&self.values, false)?,
            Shape::Vector => c.vector(&self.values)?,
            Shape::Entry => c.map_entry(&self.values[0], &self.values[1])?,
            Shape::Map | Shape::Record | Shape::Set => self.result.unwrap(),
        };
        if let Some(meta) = self.metadata {
            Ok(c.with_meta(&result, &meta)?)
        } else {
            Ok(result)
        }
    }
}
type Inner<'a> = dyn FnMut(&mut Context, &RootedValue) -> NativeResult + 'a;
fn children(c: &mut Context, form: &RootedValue, inner: &mut Inner<'_>) -> NativeResult {
    let Some(mut frame) = Traversal::new(c, form)? else {
        return Ok(form.clone());
    };
    while let Some(value) = frame.cursor.next(c)? {
        let value = inner(c, &value)?;
        frame.push(c, value)?;
    }
    frame.finish(c)
}
#[derive(Clone, Copy)]
enum Transform<'a> {
    Call(&'a RootedValue),
    Replace(&'a RootedValue),
    Keywords,
    Strings,
}
impl Transform<'_> {
    fn apply(self, c: &mut Context, form: &RootedValue) -> NativeResult {
        match self {
            Self::Call(function) => c
                .invoke(function, std::slice::from_ref(form))
                .map_err(Into::into),
            Self::Replace(replacements) => {
                if matches!(replacements.inspect(), ValueView::Nil) {
                    return Ok(form.clone());
                }
                c.realize_key(form)?;
                Ok(replacements.get(form)?.unwrap_or_else(|| form.clone()))
            }
            Self::Keywords | Self::Strings
                if matches!(form.inspect(), ValueView::Map(_) | ValueView::Record(_)) =>
            {
                // Like clojure.walk, key conversion rebuilds maps with {} and
                // intentionally does not retain the original map's metadata.
                let mut result = c.map(&[])?;
                for (key, value) in form.entries()? {
                    c.checkpoint(PollPoint::Iteration)?;
                    let key = match (self, key.inspect()) {
                        (Self::Keywords, ValueView::String(units)) => {
                            c.accounting.work(units.len() as u64)?;
                            c.charge_allocation(units.len() as u64)?;
                            c.keyword(strings::split_name(&units))
                        }
                        (Self::Strings, ValueView::Keyword(name)) => c.string(name.name()),
                        _ => key,
                    };
                    result = c.assoc(&result, &key, &value)?;
                }
                Ok(result)
            }
            _ => Ok(form.clone()),
        }
    }
}
fn visit(c: &mut Context, form: &RootedValue, transform: Transform<'_>, pre: bool) -> NativeResult {
    let base = c.active_expressions;
    let result = (|| {
        let mut frames: Vec<Traversal> = Vec::new();
        let mut pending = form.clone();
        'descend: loop {
            c.checkpoint(PollPoint::Iteration)?;
            if c.active_expressions >= c.accounting.depths().1 {
                return Err(c
                    .accounting
                    .fail(ResourceError::EvaluationDepthExceeded)
                    .into());
            }
            // Outstanding data frames also count during reentrant callbacks;
            // the explicit frame stack itself never consumes the native stack.
            c.active_expressions += 1;
            let value = if pre {
                transform.apply(c, &pending)?
            } else {
                pending
            };
            let mut result = if let Some(mut frame) = Traversal::new(c, &value)? {
                if let Some(child) = frame.cursor.next(c)? {
                    pending = child;
                    frames.push(frame);
                    continue;
                }
                frame.finish(c)?
            } else {
                value
            };
            loop {
                if !pre {
                    result = transform.apply(c, &result)?;
                }
                c.active_expressions -= 1;
                let Some(frame) = frames.last_mut() else {
                    return Ok(result);
                };
                frame.push(c, result)?;
                if let Some(child) = frame.cursor.next(c)? {
                    pending = child;
                    continue 'descend;
                }
                result = frames.pop().unwrap().finish(c)?;
            }
        }
    })();
    c.active_expressions = base;
    result
}

fn replace(c: &mut Context, a: &[RootedValue], pre: bool) -> NativeResult {
    if !matches!(a[0].inspect(), ValueView::Map(_) | ValueView::Nil) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "replacement map or nil",
        });
    }
    visit(c, &a[1], Transform::Replace(&a[0]), pre)
}
pub fn walk_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $arity:expr, $body:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.walk",
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
        def!("walk", Arity::Exact(3), |c, a| {
            let value = children(c, &a[2], &mut |c, v| {
                c.invoke(&a[0], std::slice::from_ref(v)).map_err(Into::into)
            })?;
            c.invoke(&a[1], &[value]).map_err(Into::into)
        }),
        def!("prewalk", Arity::Exact(2), |c, a| visit(
            c,
            &a[1],
            Transform::Call(&a[0]),
            true
        )),
        def!("postwalk", Arity::Exact(2), |c, a| visit(
            c,
            &a[1],
            Transform::Call(&a[0]),
            false
        )),
        def!("prewalk-replace", Arity::Exact(2), |c, a| replace(
            c, a, true
        )),
        def!("postwalk-replace", Arity::Exact(2), |c, a| replace(
            c, a, false
        )),
        def!("keywordize-keys", Arity::Exact(1), |c, a| visit(
            c,
            &a[0],
            Transform::Keywords,
            false
        )),
        def!("stringify-keys", Arity::Exact(1), |c, a| visit(
            c,
            &a[0],
            Transform::Strings,
            false
        )),
    ];
    DEFINITIONS
}
