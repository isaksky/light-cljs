//! SCI observations through compiler-backed sessions. No native interpreter API.
use super::protocol::{self, Value};
use light_cljs::{
    EvalOptions, EvalSession, HostArity, HostFunction,
    compiler_data::{Datum, DatumKind, Name},
    parser::CollectionKind,
    source_provider::{LoadRequest, SourceInput},
};
use std::{cell::RefCell, rc::Rc};

/// The bridge intentionally exports finite sequences as readable lists and
/// characters as strings. Every numeric runtime value is a JavaScript Number.
/// This is an explicit CLJS comparison projection, not exact JVM type equality.
pub fn portable(value: &Value) -> Result<Value, String> {
    Ok(match value {
        Value::Int(n) if n.unsigned_abs() <= 9_007_199_254_740_991 => {
            Value::Float((*n as f64).to_bits())
        }
        Value::Int(_) | Value::BigInt(_) | Value::Decimal(..) | Value::Ratio(..) => {
            return Err("JVM numeric observation is outside portable Number comparison".into());
        }
        Value::Char(unit) => Value::String(vec![*unit]),
        Value::List(values) | Value::Seq(values) => {
            Value::List(values.iter().map(portable).collect::<Result<_, _>>()?)
        }
        Value::Vector(values) => {
            Value::Vector(values.iter().map(portable).collect::<Result<_, _>>()?)
        }
        Value::Set(values) => {
            let mut values = values.iter().map(portable).collect::<Result<Vec<_>, _>>()?;
            values.sort_by_key(Value::wire);
            Value::Set(values)
        }
        Value::Map(entries) => {
            let mut entries = entries
                .iter()
                .map(|(k, v)| Ok((portable(k)?, portable(v)?)))
                .collect::<Result<Vec<_>, String>>()?;
            entries.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
            Value::Map(entries)
        }
        other => other.clone(),
    })
}

pub fn wire(data: &Datum) -> Result<Value, String> {
    Ok(match data.kind() {
        DatumKind::Nil => Value::Nil,
        DatumKind::Boolean(v) => Value::Bool(*v),
        DatumKind::Integer(v) => Value::Float((*v as f64).to_bits()),
        DatumKind::Double(v) => Value::Float(if v.is_nan() {
            f64::NAN.to_bits()
        } else {
            v.to_bits()
        }),
        DatumKind::Ratio { .. } => return Err("unexpected ratio in JavaScript data".into()),
        DatumKind::Character(v) => Value::String(vec![*v]),
        DatumKind::String(v) => Value::String(v.to_vec()),
        DatumKind::Symbol(v) => {
            Value::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
        }
        DatumKind::Keyword(v) => {
            Value::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
        }
        DatumKind::Collection { kind, children } => {
            let mut values = children.iter().map(wire).collect::<Result<Vec<_>, _>>()?;
            match kind {
                CollectionKind::List | CollectionKind::AnonymousFn => Value::List(values),
                CollectionKind::Vector => Value::Vector(values),
                CollectionKind::Set => {
                    values.sort_by_key(Value::wire);
                    Value::Set(values)
                }
                CollectionKind::Map => {
                    if values.len() % 2 != 0 {
                        return Err("odd map bridge output".into());
                    }
                    let mut entries: Vec<_> = values
                        .chunks_exact(2)
                        .map(|p| (p[0].clone(), p[1].clone()))
                        .collect();
                    entries.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
                    Value::Map(entries)
                }
            }
        }
        DatumKind::CompilerLiteral { .. } => {
            return Err("opaque compiler literal in SCI transport".into());
        }
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    /// Returned finite data or an owned diagnostic; diagnostics are deliberately
    /// not assigned SCI phases/categories without a compiler diagnostic API.
    pub outcome: String,
    pub effects: Vec<Value>,
    pub stdout: Vec<u16>,
    pub stderr: Vec<u16>,
}
impl Observation {
    pub fn fields(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.outcome,
            Value::Vector(self.effects.clone()).wire(),
            Value::String(self.stdout.clone()).wire(),
            Value::String(self.stderr.clone()).wire()
        )
    }
    pub fn portable(case: &protocol::Case<'_>) -> Result<Self, String> {
        let protocol::Outcome::Returned(value) = &case.outcome else {
            return Err("SCI failure requires an explicit evaluator adaptation".into());
        };
        Ok(Self {
            outcome: format!("(return {})", portable(value)?.wire()),
            effects: case
                .effects
                .iter()
                .map(portable)
                .collect::<Result<_, _>>()?,
            stdout: case.stdout.clone(),
            stderr: case.stderr.clone(),
        })
    }
}

pub fn run(id: &str, source: &str, profile: &str) -> Result<Observation, String> {
    // QuickJS stack frames contain a process-global script counter. Compare the
    // owned message only; incidental stacks are not semantic observations.
    let message = |text: &str| {
        Value::String(text.lines().next().unwrap_or("").encode_utf16().collect()).wire()
    };
    let trace = Rc::new(RefCell::new(Vec::<Datum>::new()));
    let capture = trace.clone();
    let bounded_effects = id == "sci-recur-operand-failure";
    let lazy_cycle_probe = matches!(id, "sci-deferred-lazy-self" | "sci-deferred-lazy-mutual");
    let stdout = Rc::new(RefCell::new(String::new()));
    let out = stdout.clone();
    let stderr = Rc::new(RefCell::new(String::new()));
    let err = stderr.clone();
    let mut options = EvalOptions {
        namespace: "user".into(),
        functions: vec![HostFunction {
            name: "user/observe".into(),
            arity: HostArity::Exact(1),
            callback: Rc::new(move |call, args| {
                // A historical finite-demand probe currently over-realizes an
                // infinite source. Bound its effects deterministically instead
                // of snapshotting a machine-speed-dependent deadline prefix.
                if bounded_effects && capture.borrow().len() >= 256 {
                    call.control().cancel();
                    call.control().poll()?;
                }
                let data = call.data(args[0])?;
                capture.borrow_mut().push(data);
                Ok(args[0])
            }),
        }],
        output: Some(Rc::new(move |text| {
            out.borrow_mut().push_str(text);
            Ok(())
        })),
        error_output: Some(Rc::new(move |text| {
            err.borrow_mut().push_str(text);
            Ok(())
        })),
        ..Default::default()
    };
    // Keep the default generous poll budget for cold compiler bootstrap. Only
    // the two known nonpassing lazy-cycle probes use a short backup deadline;
    // their terminal resource outcome is explicit and never a semantic pass.
    options.engine.timeout = std::time::Duration::from_secs(30);
    if lazy_cycle_probe {
        options.engine.timeout = std::time::Duration::from_secs(3);
    }
    if profile == "reader-tags" || source.contains("#t/data") {
        options.data_readers = vec![
            (Name::new(Some("t"), "tag"), "(fn [x] (inc x))".into()),
            (Name::new(Some("t"), "data"), "(fn [x] x)".into()),
            (Name::new(Some("x"), "str"), "(fn [x] (str x))".into()),
        ];
        // A compile-time tag cannot call the application callback. The identity
        // handler remains observable as a reader gap when SCI records effects.
    }
    if profile == "reader-fallback" {
        options.default_data_reader = Some("(fn [tag x] [tag x])".into());
    }
    if profile == "loading" {
        options.source_provider = Some(Rc::new(|request: &LoadRequest| {
            let namespace = String::from_utf16_lossy(request.namespace.name());
            Ok(include_str!("../reference/sci/load-sources.tsv")
                .lines()
                .filter(|line| !line.starts_with('#') && !line.is_empty())
                .find_map(|line| {
                    let fields: Vec<_> = line.split('\t').collect();
                    (fields[0] == namespace).then(|| SourceInput::new(fields[1], fields[2]))
                }))
        }));
    }
    let mut session =
        EvalSession::new(options).map_err(|e| format!("session initialization: {e}"))?;
    let outcome = match session.eval_source(&SourceInput::new("<sci-evaluator>.cljs", source)) {
        Ok(result) => {
            let data = session.data(result.value);
            session.release(result.value).map_err(|e| e.to_string())?;
            match data {
                Ok(data) => match wire(&data) {
                    Ok(value) => format!("(return {})", value.wire()),
                    Err(error) => format!("(transport {})", message(&error)),
                },
                Err(error) => format!("(transport {})", message(&error.message)),
            }
        }
        Err(error)
            if lazy_cycle_probe
                && matches!(
                    error.message.lines().next(),
                    Some(
                        "JavaScript session deadline exceeded"
                            | "JavaScript session poll budget exhausted"
                    )
                ) =>
        {
            "(resource-bound lazy-cycle)".into()
        }
        Err(error) => format!("(diagnostic {})", message(&error.message)),
    };
    // Capture realization effects before session teardown and retain only Datum.
    let effects = trace.borrow().iter().map(wire).collect::<Result<_, _>>()?;
    Ok(Observation {
        outcome,
        effects,
        stdout: stdout.borrow().encode_utf16().collect(),
        stderr: stderr.borrow().encode_utf16().collect(),
    })
}
