//! Native control macros construct owned forms. Constants in case stay data;
//! ordinary expressions flow through the existing indexed analyzer/evaluator.
use super::*;
use binding_macros::{call, fresh, invalid};

fn bind(c: &Context, name: &RootedValue, value: RootedValue, body: RootedValue) -> NativeResult {
    call(c, "let*", &[c.vector(&[name.clone(), value])?, body])
}
fn truth(c: &Context, test: RootedValue, yes: RootedValue, no: RootedValue) -> NativeResult {
    call(c, "if", &[test, yes, no])
}
fn syntax_type(expected: &'static str) -> NativeError {
    NativeError::Type {
        argument: 2,
        expected,
    }
}
pub(super) fn and(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    short_circuit(c, &args[2..], false)
}
pub(super) fn or(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    short_circuit(c, &args[2..], true)
}
fn short_circuit(c: &Context, forms: &[RootedValue], is_or: bool) -> NativeResult {
    let Some((last, rest)) = forms.split_last() else {
        return Ok(if is_or { c.nil() } else { c.boolean(true) });
    };
    let mut result = last.clone();
    for form in rest.iter().rev() {
        c.accounting.work(1)?;
        let temp = fresh(c);
        let branch = if is_or {
            truth(c, temp.clone(), temp.clone(), result)?
        } else {
            truth(c, temp.clone(), result, temp.clone())?
        };
        result = bind(c, &temp, form.clone(), branch)?;
    }
    Ok(result)
}
pub(super) fn if_not(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    truth(
        c,
        call(c, "not", &args[2..3])?,
        args[3].clone(),
        args.get(4).cloned().unwrap_or(c.nil()),
    )
}
pub(super) fn when(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    truth(c, args[2].clone(), call(c, "do", &args[3..])?, c.nil())
}
pub(super) fn when_not(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    truth(c, args[2].clone(), c.nil(), call(c, "do", &args[3..])?)
}
fn conditional_binding(
    c: &Context,
    args: &[RootedValue],
    some: bool,
    when: bool,
    first: bool,
) -> NativeResult {
    if !matches!(args[2].inspect(), ValueView::Vector(_)) {
        return Err(invalid("conditional binding requires a vector"));
    }
    let bindings = args[2].elements()?;
    if bindings.len() != 2 {
        return Err(invalid("conditional binding requires exactly two forms"));
    }
    let temp = fresh(c);
    let input = if first {
        call(c, "seq", &bindings[1..])?
    } else {
        bindings[1].clone()
    };
    let value = if first {
        call(c, "first", std::slice::from_ref(&temp))?
    } else {
        temp.clone()
    };
    let mut body = vec![c.vector(&[bindings[0].clone(), value])?];
    if when {
        body.extend_from_slice(&args[3..]);
    } else {
        body.push(args[3].clone());
    }
    let yes = call(c, "let", &body)?;
    let no = if when {
        c.nil()
    } else {
        args.get(4).cloned().unwrap_or(c.nil())
    };
    let branch = if some {
        truth(c, call(c, "nil?", std::slice::from_ref(&temp))?, no, yes)?
    } else {
        truth(c, temp.clone(), yes, no)?
    };
    bind(c, &temp, input, branch)
}
pub(super) fn if_let(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_binding(c, a, false, false, false)
}
pub(super) fn if_some(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_binding(c, a, true, false, false)
}
pub(super) fn when_let(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_binding(c, a, false, true, false)
}
pub(super) fn when_some(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_binding(c, a, true, true, false)
}
pub(super) fn when_first(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_binding(c, a, false, true, true)
}
pub(super) fn cond(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let clauses = &args[2..];
    if clauses.is_empty() {
        return Ok(c.nil());
    }
    if clauses.len() == 1 {
        return Err(syntax_type("an even number of cond forms"));
    }
    // Leave remaining clauses for their own expansion after earlier branches
    // are analyzed, preserving macro effects even before a malformed tail.
    truth(
        c,
        clauses[0].clone(),
        clauses[1].clone(),
        call(c, "cond", &clauses[2..])?,
    )
}
fn no_match(c: &Context, value: RootedValue) -> NativeResult {
    // The primitive's absent default throws a catchable language failure.
    call(c, "case*", &[value, c.vector(&[])?])
}
pub(super) fn condp(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let pred = fresh(c);
    let value = fresh(c);
    let mut clauses = Vec::new();
    let mut rest = &args[4..];
    while rest.len() >= 2 {
        c.accounting.work(1)?;
        c.charge_allocation(3)?;
        let arrow =
            matches!(rest[1].inspect(), ValueView::Keyword(n) if n == Name::new(None, ">>"));
        // Pinned SCI interprets a dangling :>> as an ordinary two-form clause.
        let width = if arrow && rest.len() >= 3 { 3 } else { 2 };
        clauses.push((&rest[..width], width == 3));
        rest = &rest[width..];
    }
    let mut result = if let Some(default) = rest.first() {
        default.clone()
    } else {
        no_match(c, value.clone())?
    };
    for (clause, arrow) in clauses.into_iter().rev() {
        let test = c.list(&[pred.clone(), clause[0].clone(), value.clone()])?;
        result = if arrow {
            let matched = fresh(c);
            bind(
                c,
                &matched,
                test,
                truth(
                    c,
                    matched.clone(),
                    c.list(&[clause[2].clone(), matched.clone()])?,
                    result,
                )?,
            )?
        } else {
            truth(c, test, clause[1].clone(), result)?
        };
    }
    call(
        c,
        "let*",
        &[
            c.vector(&[pred, args[2].clone(), value, args[3].clone()])?,
            result,
        ],
    )
}
fn step(c: &Context, value: RootedValue, form: &RootedValue, last: bool) -> NativeResult {
    let mut forms = if matches!(form.inspect(), ValueView::List(_) | ValueView::Seq(_)) {
        let mut forms = form.elements()?;
        if forms.is_empty() {
            forms.push(c.nil());
        }
        forms
    } else {
        vec![form.clone()]
    };
    c.charge_allocation(forms.len() as u64 + 1)?;
    if last {
        forms.push(value);
    } else {
        forms.insert(1, value);
    }
    let result = c.list(&forms)?;
    Ok(if let Some(meta) = form.metadata() {
        c.with_meta(&result, &meta)?
    } else {
        result
    })
}
fn thread(c: &Context, args: &[RootedValue], last: bool) -> NativeResult {
    let mut result = args[2].clone();
    for form in &args[3..] {
        c.accounting.work(1)?;
        result = step(c, result, form, last)?;
    }
    Ok(result)
}
pub(super) fn thread_first(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    thread(c, a, false)
}
pub(super) fn thread_last(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    thread(c, a, true)
}
pub(super) fn as_thread(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let name = &args[3];
    let mut bindings = vec![name.clone(), args[2].clone()];
    let (result, forms) = args[4..]
        .split_last()
        .map_or((name, &[][..]), |(last, rest)| (last, rest));
    for form in forms {
        c.accounting.work(1)?;
        c.charge_allocation(2)?;
        bindings.extend([name.clone(), form.clone()]);
    }
    call(c, "let", &[c.vector(&bindings)?, result.clone()])
}
fn conditional_thread(c: &Context, args: &[RootedValue], last: bool, some: bool) -> NativeResult {
    let clauses = &args[3..];
    if !some && !clauses.len().is_multiple_of(2) {
        return Err(invalid("conditional threading requires test/step pairs"));
    }
    let temp = fresh(c);
    let mut bindings = vec![temp.clone(), args[2].clone()];
    let width = if some { 1 } else { 2 };
    let mut result = temp.clone();
    for clause in clauses.chunks(width) {
        c.accounting.work(1)?;
        if bindings.len() > 2 || !result.same_identity(&temp) {
            c.charge_allocation(2)?;
            bindings.extend([temp.clone(), result]);
        }
        let threaded = step(c, temp.clone(), clause.last().unwrap(), last)?;
        result = if some {
            truth(
                c,
                call(c, "nil?", std::slice::from_ref(&temp))?,
                c.nil(),
                threaded,
            )?
        } else {
            truth(c, clause[0].clone(), threaded, temp.clone())?
        };
    }
    call(c, "let*", &[c.vector(&bindings)?, result])
}
pub(super) fn cond_thread_first(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_thread(c, a, false, false)
}
pub(super) fn cond_thread_last(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_thread(c, a, true, false)
}
pub(super) fn some_thread_first(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_thread(c, a, false, true)
}
pub(super) fn some_thread_last(c: &mut Context, a: &[RootedValue]) -> NativeResult {
    conditional_thread(c, a, true, true)
}
pub(super) fn comment(c: &mut Context, _: &[RootedValue]) -> NativeResult {
    Ok(c.nil())
}
pub(super) fn assert(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let failure = call(
        c,
        "throw",
        &[call(
            c,
            "AssertionError.",
            &[c.string("Assert failed".encode_utf16().collect::<Vec<_>>())],
        )?],
    )?;
    // The profile has fixed text until a general printer is implemented, but
    // the optional message remains a real expression, evaluated only on failure.
    let failure = if let Some(message) = args.get(3) {
        call(c, "do", &[message.clone(), failure])?
    } else {
        failure
    };
    truth(c, args[2].clone(), c.nil(), failure)
}
pub(super) fn case(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let clauses = &args[3..];
    let (pairs, default) = clauses.as_chunks::<2>();
    let mut branches = Vec::new();
    let mut seen = c.map(&[])?;
    for pair in pairs {
        c.accounting.work(1)?;
        let constants = if matches!(pair[0].inspect(), ValueView::List(_) | ValueView::Seq(_)) {
            pair[0].elements()?
        } else {
            vec![pair[0].clone()]
        };
        for constant in constants {
            c.accounting.work(1)?;
            c.charge_allocation(2)?;
            if seen.get(&constant)?.is_some() {
                return Err(syntax_type("distinct case test constants"));
            }
            seen = c.assoc(&seen, &constant, &c.boolean(true))?;
            branches.extend([constant, pair[1].clone()]);
        }
    }
    let mut forms = vec![args[2].clone(), c.vector(&branches)?];
    forms.extend_from_slice(default);
    call(c, "case*", &forms)
}
