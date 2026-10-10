//! Comprehensions lower to indexed loops and ordinary traced lazy closures.
//! The unchunked expansion follows SCI's generator/modifier order, without
//! collecting effectful iterations or retaining an interpreter frame in a tail.
use super::*;
use binding_macros::{call, fresh, invalid};

#[derive(Clone, Copy)]
enum Modifier {
    Let,
    When,
    While,
}
struct Group {
    binding: RootedValue,
    input: RootedValue,
    modifiers: Vec<(Modifier, RootedValue)>,
}
type Modifiers = Vec<(Modifier, RootedValue)>;

fn pairs(value: &RootedValue) -> Result<Vec<RootedValue>, NativeError> {
    if !matches!(value.inspect(), ValueView::Vector(_)) {
        return Err(invalid("iteration bindings must be a vector"));
    }
    let forms = value.elements()?;
    if !forms.len().is_multiple_of(2) {
        return Err(invalid(
            "iteration bindings require an even number of forms",
        ));
    }
    Ok(forms)
}
fn groups(c: &Context, value: &RootedValue) -> Result<(Modifiers, Vec<Group>), NativeError> {
    let mut prefix = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for pair in pairs(value)?.as_chunks::<2>().0 {
        c.accounting.work(1)?;
        c.charge_allocation(3)?;
        if let ValueView::Keyword(name) = pair[0].inspect() {
            let modifier = if name == Name::new(None, "let") {
                // Validate before producing any executable form. Destructuring
                // itself still uses the normal source-aware binding expander.
                pairs(&pair[1])?;
                Modifier::Let
            } else if name == Name::new(None, "when") {
                Modifier::When
            } else if name == Name::new(None, "while") {
                Modifier::While
            } else {
                return Err(invalid("iteration modifier must be :let, :when or :while"));
            };
            groups
                .last_mut()
                .map_or(&mut prefix, |g| &mut g.modifiers)
                .push((modifier, pair[1].clone()));
        } else {
            groups.push(Group {
                binding: pair[0].clone(),
                input: pair[1].clone(),
                modifiers: Vec::new(),
            });
        }
    }
    Ok((prefix, groups))
}
fn bind(c: &Context, name: RootedValue, value: RootedValue, body: RootedValue) -> NativeResult {
    call(c, "let*", &[c.vector(&[name, value])?, body])
}
fn modifiers(
    c: &Context,
    modifiers: &[(Modifier, RootedValue)],
    mut body: RootedValue,
    advance: &RootedValue,
    prefix: bool,
) -> NativeResult {
    for (modifier, form) in modifiers.iter().rev() {
        c.accounting.work(1)?;
        body = match modifier {
            Modifier::Let => call(c, "let", &[form.clone(), body])?,
            Modifier::When | Modifier::While => {
                // With no enclosing generator SCI uses nil as the continuation.
                // A conditional prefix thus discards its admitted body's value.
                let yes = if prefix {
                    call(c, "do", &[body, c.nil()])?
                } else {
                    body
                };
                let no = match modifier {
                    Modifier::When => advance.clone(),
                    _ => c.nil(),
                };
                call(c, "if", &[form.clone(), yes, no])?
            }
        };
    }
    Ok(body)
}

pub(super) fn for_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let (prefix, groups) = groups(c, &args[2])?;
    if !prefix.is_empty() || groups.is_empty() {
        return Err(invalid("for requires a generator before any modifiers"));
    }
    // Build inside out, avoiding recursive macro-construction on the Rust stack.
    let mut nested: Option<RootedValue> = None;
    for (index, group) in groups.iter().enumerate().rev() {
        c.accounting.work(1)?;
        let iter = fresh(c);
        let xs = fresh(c);
        let rest = call(c, "rest", std::slice::from_ref(&xs))?;
        let advance = call(c, "recur", std::slice::from_ref(&rest))?;
        let tail = c.list(&[iter.clone(), rest])?;
        let body = if let Some(nested) = nested {
            let child = fresh(c);
            let results = fresh(c);
            let input = c.list(&[child.clone(), groups[index + 1].input.clone()])?;
            bind(
                c,
                child,
                nested,
                bind(
                    c,
                    results.clone(),
                    call(c, "seq", &[input])?,
                    call(
                        c,
                        "if",
                        &[
                            results.clone(),
                            call(c, "concat", &[results, tail])?,
                            advance.clone(),
                        ],
                    )?,
                )?,
            )?
        } else {
            call(c, "cons", &[args[3].clone(), tail])?
        };
        let body = modifiers(c, &group.modifiers, body, &advance, false)?;
        let body = call(
            c,
            "let",
            &[
                c.vector(&[
                    group.binding.clone(),
                    call(c, "first", std::slice::from_ref(&xs))?,
                ])?,
                body,
            ],
        )?;
        let body = bind(
            c,
            xs.clone(),
            call(c, "seq", std::slice::from_ref(&xs))?,
            call(c, "if", &[xs.clone(), body, c.nil()])?,
        )?;
        let body = call(c, "loop*", &[c.vector(&[xs.clone(), xs.clone()])?, body])?;
        nested = Some(call(
            c,
            "fn*",
            &[iter, c.vector(&[xs])?, call(c, "lazy-seq", &[body])?],
        )?);
    }
    let iter = fresh(c);
    bind(
        c,
        iter.clone(),
        nested.unwrap(),
        c.list(&[iter, groups[0].input.clone()])?,
    )
}

pub(super) fn doseq(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let (prefix, groups) = groups(c, &args[2])?;
    let mut body = call(c, "do", &args[3..])?;
    for group in groups.iter().rev() {
        c.accounting.work(1)?;
        let xs = fresh(c);
        let advance = call(c, "recur", &[call(c, "next", std::slice::from_ref(&xs))?])?;
        body = call(c, "do", &[body, advance.clone()])?;
        body = modifiers(c, &group.modifiers, body, &advance, false)?;
        body = call(
            c,
            "let",
            &[
                c.vector(&[
                    group.binding.clone(),
                    call(c, "first", std::slice::from_ref(&xs))?,
                ])?,
                body,
            ],
        )?;
        body = call(c, "if", &[xs.clone(), body, c.nil()])?;
        body = call(
            c,
            "loop*",
            &[
                c.vector(&[xs, call(c, "seq", std::slice::from_ref(&group.input))?])?,
                body,
            ],
        )?;
    }
    modifiers(c, &prefix, body, &c.nil(), true)
}

pub(super) fn dotimes(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let bindings = pairs(&args[2])?;
    if bindings.len() != 2 {
        return Err(invalid("dotimes requires exactly two binding forms"));
    }
    let limit = fresh(c);
    let mut body = args[3..].to_vec();
    body.push(call(c, "recur", &[call(c, "inc", &bindings[..1])?])?);
    let test = call(c, "<", &[bindings[0].clone(), limit.clone()])?;
    let body = call(c, "if", &[test, call(c, "do", &body)?, c.nil()])?;
    let body = call(
        c,
        "loop",
        &[c.vector(&[bindings[0].clone(), c.integer(0)])?, body],
    )?;
    let bound = if c.macros.compiler_expansion {
        bindings[1].clone()
    } else {
        call(c, "long", &bindings[1..])?
    };
    bind(c, limit, bound, body)
}
pub(super) fn while_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let mut body = args[3..].to_vec();
    body.push(call(c, "recur", &[])?);
    call(
        c,
        "loop*",
        &[
            c.vector(&[])?,
            call(c, "if", &[args[2].clone(), call(c, "do", &body)?, c.nil()])?,
        ],
    )
}
