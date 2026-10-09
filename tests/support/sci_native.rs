//! Shared exact SCI transport for whole-program native conformance.
use super::protocol;
use light_cljs::interpreter::*;
use std::{cell::RefCell, collections::HashMap};
pub fn wire(context: &mut Context, value: &RootedValue) -> Result<protocol::Value, String> {
    fn encode(
        c: &mut Context,
        value: &RootedValue,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<protocol::Value, String> {
        use protocol::Value as V;
        *nodes += 1;
        if depth > 128 || *nodes > 10_000 {
            return Err("native SCI transport limit".into());
        }
        let mut items = |values: Vec<RootedValue>| {
            values
                .iter()
                .map(|v| encode(c, v, depth + 1, nodes))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match value.inspect() {
            ValueView::Nil => V::Nil,
            ValueView::Boolean(v) => V::Bool(v),
            ValueView::Integer(v) => V::Int(v),
            ValueView::Double(v) => V::Float(if v.is_nan() {
                f64::NAN.to_bits()
            } else {
                v.to_bits()
            }),
            ValueView::Ratio {
                numerator,
                denominator,
            } => V::Ratio(numerator, denominator),
            ValueView::Character(v) => V::Char(v),
            ValueView::String(v) => V::String(v.to_vec()),
            ValueView::Symbol(v) => {
                V::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
            }
            ValueView::Keyword(v) => {
                V::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
            }
            ValueView::List(_) => V::List(items(value.elements().map_err(|e| format!("{e:?}"))?)?),
            ValueView::Seq(_) => V::Seq(items(value.elements().map_err(|e| format!("{e:?}"))?)?),
            ValueView::Vector(_) => {
                V::Vector(items(value.elements().map_err(|e| format!("{e:?}"))?)?)
            }
            ValueView::LazySeq => {
                let values = c
                    .sequence_elements(value)
                    .map_err(|e| format!("native transport realization: {e}"))?;
                V::Seq(
                    values
                        .iter()
                        .map(|v| encode(c, v, depth + 1, nodes))
                        .collect::<Result<_, _>>()?,
                )
            }
            ValueView::Map(_) => {
                let mut values = value
                    .entries()
                    .map_err(|e| format!("{e:?}"))?
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            encode(c, k, depth + 1, nodes)?,
                            encode(c, v, depth + 1, nodes)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                values.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
                V::Map(values)
            }
            ValueView::Set(_) => {
                let mut values = items(value.elements().map_err(|e| format!("{e:?}"))?)?;
                values.sort_by_key(V::wire);
                V::Set(values)
            }
            other => return Err(format!("unsupported native SCI transport value: {other:?}")),
        })
    }
    encode(context, value, 0, &mut 0)
}

thread_local! { static TRACE: RefCell<Vec<RootedValue>> = const { RefCell::new(Vec::new()) }; }
static OBSERVE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "observe",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |context, args| {
        TRACE.with(|trace| trace.borrow_mut().push(args[0].clone()));
        context.collect_garbage();
        Ok(args[0].clone())
    },
};

#[derive(Debug, PartialEq)]
pub struct Observation {
    pub outcome: protocol::Outcome,
    pub effects: Vec<protocol::Value>,
    pub stdout: Vec<u16>,
    pub stderr: Vec<u16>,
}
impl Observation {
    pub fn expected(case: &protocol::Case<'_>) -> Self {
        Self {
            outcome: case.outcome.clone(),
            effects: case.effects.clone(),
            stdout: case.stdout.clone(),
            stderr: case.stderr.clone(),
        }
    }
}

pub fn run(source: &str, profile: &str) -> Result<Observation, String> {
    let mut c = Context::new();
    c.set_resource_limits(ResourceLimits {
        fuel: Some(2_000_000),
        allocation: Some(4_000_000),
        output: Some(100_000),
        ..Default::default()
    });
    c.register_native(&OBSERVE);
    if profile == "loading" {
        let sources: HashMap<_, _> = include_str!("../reference/sci/load-sources.tsv")
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|line| {
                let fields: Vec<_> = line.split('\t').collect();
                (
                    Name::new(None, fields[0]),
                    SourceInput::new(fields[1], fields[2]),
                )
            })
            .collect();
        c.set_source_provider(move |request: &LoadRequest| {
            Ok(sources.get(&request.namespace).cloned())
        });
    }
    if profile == "reader-bb" {
        c.set_reader_features([Name::new(None, "bb")]);
    }
    if profile == "reader-tags" {
        c.register_data_reader(
            Name::new(Some("t"), "tag"),
            builtin_definitions()
                .iter()
                .find(|d| d.metadata.name == "inc")
                .unwrap(),
        );
        c.register_data_reader(Name::new(Some("t"), "data"), &OBSERVE);
        c.register_data_reader(
            Name::new(Some("x"), "str"),
            printing_definitions()
                .iter()
                .find(|d| d.metadata.name == "str")
                .unwrap(),
        );
    }
    static FALLBACK: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "fallback",
            arity: Arity::Exact(2),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, a| Ok(c.vector(a)?),
    };
    if profile == "reader-fallback" {
        c.set_default_data_reader(Some(&FALLBACK));
    }
    let stdout = OutputBuffer::default();
    let stderr = OutputBuffer::default();
    let out = c.output_stream(stdout.clone());
    let err = c.output_stream(stderr.clone());
    let _guard = c.bind_io(None, Some(&out), Some(&err)).unwrap();
    TRACE.with(|t| t.borrow_mut().clear());
    let outcome = match c.eval_string(source) {
        Ok(value) => protocol::Outcome::Returned(wire(&mut c, &value)?),
        Err(error) => failure(&error)?,
    };
    let trace = TRACE.with(|t| std::mem::take(&mut *t.borrow_mut()));
    let effects = trace
        .iter()
        .map(|v| wire(&mut c, v))
        .collect::<Result<_, _>>()?;
    TRACE.with(|t| t.borrow_mut().clear());
    Ok(Observation {
        outcome,
        effects,
        stdout: stdout.units(),
        stderr: stderr.units(),
    })
}

fn failure(error: &EvalError) -> Result<protocol::Outcome, String> {
    use EvalErrorKind as E;
    use protocol::{Category as C, Phase as P};
    let phase = match error.phase {
        EvalPhase::Read => P::Read,
        EvalPhase::Analysis => P::Analysis,
        EvalPhase::Execution | EvalPhase::Macroexpansion => P::Execution,
    };
    let category = match &error.kind {
        E::Read(_) => C::Reader,
        E::UnresolvedSymbol(_) => C::UnresolvedSymbol,
        E::InvalidForm(_) => C::InvalidForm,
        E::Arity { .. }
        | E::Native {
            error: NativeError::Arity { .. },
            ..
        } => C::Arity,
        E::Native {
            error:
                NativeError::StringBounds { .. }
                | NativeError::Collection(CollectionError::InvalidIndex),
            ..
        } => C::Bounds,
        E::Native {
            error: NativeError::Arithmetic(ArithmeticError::ConversionRange),
            ..
        } => C::Type,
        E::Native {
            error: NativeError::Arithmetic(_),
            ..
        } => C::Arithmetic,
        E::DuplicateMapKey | E::DuplicateSetMember => C::DuplicateKey,
        E::NotCallable
        | E::NoMatchingClause
        | E::Collection(_)
        | E::Native {
            error: NativeError::Type { .. } | NativeError::Collection(_),
            ..
        } => C::Type,
        E::Thrown(ExceptionCategory::ExceptionInfo) => C::ExceptionInfo,
        _ => return Err(format!("unclassified native failure: {error}")),
    };
    Ok(protocol::Outcome::Error { phase, category })
}
