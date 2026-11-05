//! Compile postfix Unicode sets to disjoint intervals and finite strings.
//! Every operation uses the matcher's compilation budget; no native recursion.
use super::case_fold::{SIMPLE_FOLD, canonicalize};
use super::ir::{ClassEscape, ClassMember, ClassSetItem};
use super::matcher::{Budget, MatchError};

type Ranges = Vec<(u32, u32)>;

#[derive(Default)]
pub(super) struct Set {
    pub ranges: Ranges,
    // Sorted lexically and unique during algebra; longest-first after compile.
    pub strings: Vec<Vec<u32>>,
}

#[derive(Clone, Copy)]
enum Op {
    Union,
    Intersection,
    Subtraction,
}

fn normalize(mut ranges: Ranges, budget: &mut Budget) -> Result<Ranges, MatchError> {
    // Reserve the comparison bound before sorting.
    budget.charge(sort_cost(ranges.len()))?;
    ranges.sort_unstable();
    let mut out: Ranges = Vec::new();
    for (a, b) in ranges {
        budget.charge(1)?;
        if let Some(last) = out.last_mut()
            && a <= last.1 + 1
        {
            last.1 = last.1.max(b);
        } else {
            out.push((a, b));
        }
    }
    Ok(out)
}

fn sort_cost(len: usize) -> usize {
    len.saturating_mul(len.checked_ilog2().unwrap_or(0) as usize + 1)
}

fn ranges_op(
    a: &[(u32, u32)],
    b: &[(u32, u32)],
    op: Op,
    budget: &mut Budget,
) -> Result<Ranges, MatchError> {
    if matches!(op, Op::Union) {
        budget.charge(a.len() + b.len())?;
        let mut all = a.to_vec();
        all.extend_from_slice(b);
        return normalize(all, budget);
    }
    let mut out = Vec::new();
    let mut j = 0;
    for &(start, end) in a {
        budget.charge(1)?;
        while j < b.len() && b[j].1 < start {
            budget.charge(1)?;
            j += 1;
        }
        let mut k = j;
        let mut cursor = start;
        while k < b.len() && b[k].0 <= end {
            budget.charge(1)?;
            let (lo, hi) = b[k];
            if matches!(op, Op::Intersection) {
                out.push((start.max(lo), end.min(hi)));
            } else {
                if cursor < lo {
                    out.push((cursor, end.min(lo - 1)));
                }
                cursor = cursor.max(hi + 1);
                if cursor > end {
                    break;
                }
            }
            k += 1;
        }
        if matches!(op, Op::Subtraction) && cursor <= end {
            out.push((cursor, end));
        }
    }
    Ok(out)
}

fn fold(ranges: Ranges, budget: &mut Budget) -> Result<Ranges, MatchError> {
    // Canonical images: remove nonidentity sources and add their targets.
    // The shared property tables supply raw membership; complement follows fold.
    budget.charge(SIMPLE_FOLD.len())?;
    let sources: Ranges = SIMPLE_FOLD.iter().map(|&(s, _)| (s, s)).collect();
    let mut out = ranges_op(&ranges, &sources, Op::Subtraction, budget)?;
    let mut j = 0;
    for &(source, target) in SIMPLE_FOLD {
        budget.charge(1)?;
        while j < ranges.len() && ranges[j].1 < source {
            budget.charge(1)?;
            j += 1;
        }
        if ranges
            .get(j)
            .is_some_and(|&(a, b)| a <= source && source <= b)
        {
            out.push((target, target));
        }
    }
    normalize(out, budget)
}

fn member(m: &ClassMember, ignore_case: bool, budget: &mut Budget) -> Result<Set, MatchError> {
    if let ClassMember::StringProperty(property) = m {
        budget.charge(property.ranges().len())?;
        let mut strings = Vec::new();
        let mut max_len = 0;
        for s in property.strings() {
            budget.charge(s.len() + 1)?;
            max_len = max_len.max(s.len());
            strings.push(
                s.iter()
                    .map(|&c| {
                        if ignore_case {
                            canonicalize(c, true)
                        } else {
                            c
                        }
                    })
                    .collect::<Vec<_>>(),
            );
        }
        // Comparisons inspect at most max_len scalars; charge before sorting.
        budget.charge(sort_cost(strings.len()).saturating_mul(max_len))?;
        strings.sort_unstable();
        budget.charge(strings.len().saturating_mul(max_len))?;
        strings.dedup();
        let ranges = property.ranges().to_vec();
        return Ok(Set {
            ranges: if ignore_case {
                fold(ranges, budget)?
            } else {
                ranges
            },
            strings,
        });
    }
    let (ranges, inverted) = match m {
        ClassMember::StringProperty(_) => unreachable!(),
        ClassMember::Literal(c) => {
            if *c > 0x10ffff {
                return Err(MatchError::InvalidPattern);
            }
            let c = if ignore_case {
                canonicalize(*c, true)
            } else {
                *c
            };
            return Ok(Set {
                ranges: vec![(c, c)],
                strings: Vec::new(),
            });
        }
        ClassMember::Range(a, b) => (vec![(*a, *b)], false),
        ClassMember::Property { property, inverted } => {
            budget.charge(property.ranges().len())?;
            (property.ranges().to_vec(), *inverted)
        }
        ClassMember::Escape(e) => match e {
            ClassEscape::Digit | ClassEscape::NotDigit => {
                (vec![(48, 57)], matches!(e, ClassEscape::NotDigit))
            }
            ClassEscape::Word | ClassEscape::NotWord => (
                vec![(48, 57), (65, 90), (95, 95), (97, 122)],
                matches!(e, ClassEscape::NotWord),
            ),
            ClassEscape::Space | ClassEscape::NotSpace => (
                vec![
                    (9, 13),
                    (32, 32),
                    (0xa0, 0xa0),
                    (0x1680, 0x1680),
                    (0x2000, 0x200a),
                    (0x2028, 0x2029),
                    (0x202f, 0x202f),
                    (0x205f, 0x205f),
                    (0x3000, 0x3000),
                    (0xfeff, 0xfeff),
                ],
                matches!(e, ClassEscape::NotSpace),
            ),
        },
    };
    if ranges.iter().any(|&(a, b)| a > b || b > 0x10ffff) {
        return Err(MatchError::InvalidPattern);
    }
    let mut ranges = if ignore_case {
        fold(ranges, budget)?
    } else {
        ranges
    };
    if inverted {
        ranges = ranges_op(&[(0, 0x10ffff)], &ranges, Op::Subtraction, budget)?;
    }
    Ok(Set {
        ranges,
        strings: Vec::new(),
    })
}

fn combine(a: Set, b: Set, op: Op, budget: &mut Budget) -> Result<Set, MatchError> {
    let ranges = ranges_op(&a.ranges, &b.ranges, op, budget)?;
    let mut strings = Vec::new();
    let mut a = a.strings.into_iter().peekable();
    let mut b = b.strings.into_iter().peekable();
    while let (Some(left), Some(right)) = (a.peek(), b.peek()) {
        budget.charge(left.len().min(right.len()) + 1)?;
        match left.cmp(right) {
            std::cmp::Ordering::Less => {
                let s = a.next().unwrap();
                if !matches!(op, Op::Intersection) {
                    strings.push(s);
                }
            }
            std::cmp::Ordering::Greater => {
                let s = b.next().unwrap();
                if matches!(op, Op::Union) {
                    strings.push(s);
                }
            }
            std::cmp::Ordering::Equal => {
                let s = a.next().unwrap();
                b.next();
                if !matches!(op, Op::Subtraction) {
                    strings.push(s);
                }
            }
        }
    }
    if !matches!(op, Op::Intersection) {
        for s in a {
            budget.charge(1)?;
            strings.push(s);
        }
    }
    if matches!(op, Op::Union) {
        for s in b {
            budget.charge(1)?;
            strings.push(s);
        }
    }
    Ok(Set { ranges, strings })
}

pub(super) fn validate(items: &[ClassSetItem], budget: &mut Budget) -> Result<(), MatchError> {
    let mut stack = Vec::new();
    for item in items {
        budget.charge(1)?;
        match item {
            ClassSetItem::Empty => stack.push(false),
            ClassSetItem::Member(member) => {
                if match member {
                    ClassMember::Literal(c) => *c > 0x10ffff,
                    ClassMember::Range(a, b) => a > b || *b > 0x10ffff,
                    _ => false,
                } {
                    return Err(MatchError::InvalidPattern);
                }
                stack.push(matches!(member, ClassMember::StringProperty(_)));
            }
            ClassSetItem::String(s) => {
                budget.charge(s.len())?;
                if s.iter().any(|&c| c > 0x10ffff) {
                    return Err(MatchError::InvalidPattern);
                }
                stack.push(s.len() != 1);
            }
            ClassSetItem::Complement => {
                if stack.last() != Some(&false) {
                    return Err(MatchError::InvalidPattern);
                }
            }
            op => {
                let b = stack.pop().ok_or(MatchError::InvalidPattern)?;
                let a = stack.pop().ok_or(MatchError::InvalidPattern)?;
                stack.push(match op {
                    ClassSetItem::Union => a || b,
                    ClassSetItem::Intersection => a && b,
                    ClassSetItem::Subtraction => a,
                    _ => unreachable!(),
                });
            }
        }
    }
    if stack.len() != 1 {
        return Err(MatchError::InvalidPattern);
    }
    Ok(())
}

pub(super) fn compile(
    items: &[ClassSetItem],
    ignore_case: bool,
    budget: &mut Budget,
) -> Result<Set, MatchError> {
    let mut stack: Vec<Set> = Vec::new();
    for item in items {
        budget.charge(1)?;
        match item {
            ClassSetItem::Empty => stack.push(Set::default()),
            ClassSetItem::Member(m) => stack.push(member(m, ignore_case, budget)?),
            ClassSetItem::String(s) => {
                budget.charge(s.len())?;
                if s.iter().any(|&c| c > 0x10ffff) {
                    return Err(MatchError::InvalidPattern);
                }
                if s.len() == 1 {
                    stack.push(member(&ClassMember::Literal(s[0]), ignore_case, budget)?);
                } else {
                    stack.push(Set {
                        ranges: Vec::new(),
                        strings: vec![
                            s.iter()
                                .map(|&c| {
                                    if ignore_case {
                                        canonicalize(c, true)
                                    } else {
                                        c
                                    }
                                })
                                .collect(),
                        ],
                    });
                }
            }
            ClassSetItem::Complement => {
                let set = stack.last_mut().ok_or(MatchError::InvalidPattern)?;
                if !set.strings.is_empty() {
                    return Err(MatchError::InvalidPattern);
                }
                set.ranges = ranges_op(&[(0, 0x10ffff)], &set.ranges, Op::Subtraction, budget)?;
            }
            op => {
                let b = stack.pop().ok_or(MatchError::InvalidPattern)?;
                let a = stack.pop().ok_or(MatchError::InvalidPattern)?;
                let op = match op {
                    ClassSetItem::Union => Op::Union,
                    ClassSetItem::Intersection => Op::Intersection,
                    ClassSetItem::Subtraction => Op::Subtraction,
                    _ => unreachable!(),
                };
                stack.push(combine(a, b, op, budget)?);
            }
        }
    }
    if stack.len() != 1 {
        return Err(MatchError::InvalidPattern);
    }
    let mut set = stack.pop().unwrap();
    budget.charge(sort_cost(set.strings.len()))?;
    set.strings.sort_by_key(|s| std::cmp::Reverse(s.len()));
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regexp::property;

    #[test]
    fn folded_property_algebra_covers_entire_code_point_domain() {
        let lower = property::lookup("Lower").unwrap();
        let items = [
            ClassSetItem::Member(ClassMember::Property {
                property: lower,
                inverted: true,
            }),
            ClassSetItem::Member(ClassMember::Range(65, 90)),
            ClassSetItem::Union,
        ];
        let set = compile(
            &items,
            true,
            &mut Budget::new(super::super::matcher::MAX_MATCH_STEPS),
        )
        .unwrap();
        for cp in 0..=0x10ffff {
            let folded = canonicalize(cp, true);
            let i = set.ranges.partition_point(|&(_, b)| b < folded);
            let actual = set.ranges.get(i).is_some_and(|&(a, _)| a <= folded);
            let expected = !lower.contains(folded, false, true) || (97..=122).contains(&folded);
            assert_eq!(actual, expected, "U+{cp:04X}");
        }
    }
}
