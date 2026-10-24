//! Bounded regular expression matching over UTF-16 strings, with Unicode mode.
//!
//! Compile the parser's arena once, then call [`exec`]. Matching is a leaf
//! operation: it never calls JavaScript or retains a native stack continuation.
//! Exhaustion is an error, never a failed match; the RegExp object layer must
//! turn it into a catchable RangeError. See [`MAX_MATCH_STEPS`].

use std::{fmt, ops::Range};

use super::case_fold::{SIMPLE_FOLD, canonicalize};
use super::ir::*;

/// Work allowance for one complete search, including all candidate positions,
/// lookaheads, backtracking, capture writes/restores and code-unit comparisons.
/// Compilation has a separate allowance of the same size. This also bounds the
/// explicit stacks; no native recursion depends on pattern or input depth.
pub const MAX_MATCH_STEPS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchError {
    LimitExceeded,
    Unsupported(&'static str),
    InvalidPattern,
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded => write!(f, "RangeError: regular expression step limit exceeded"),
            Self::Unsupported(feature) => {
                write!(f, "unsupported regular expression feature: {feature}")
            }
            Self::InvalidPattern => write!(f, "invalid regular expression arena"),
        }
    }
}
impl std::error::Error for MatchError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Captures {
    /// Start of the whole match, counted in UTF-16 code units.
    pub index: usize,
    /// Group zero is the whole match. Undefined captures are None. A capture in
    /// positive lookahead may extend past the whole match's end.
    pub groups: Vec<Option<Range<usize>>>,
}

/// Immutable bytecode with no VM heap or GC edges. Only [`compile`] constructs it.
#[derive(Clone, Debug)]
pub struct Program {
    code: Vec<Instruction>,
    captures: usize,
    repeats: usize,
    sticky: bool,
    unicode: bool,
}

#[derive(Clone, Debug)]
enum Instruction {
    Char(u32, bool),
    Dot(bool),
    Class(CharacterClass, bool),
    Assert(Assertion, bool, bool),
    Backreference(Vec<usize>, bool),
    Start(usize),
    End(usize),
    Clear(usize, usize),
    Split(usize),
    Jump(usize),
    RepeatInit(usize),
    RepeatHead {
        slot: usize,
        min: u64,
        max: Option<u64>,
        greedy: bool,
        body: usize,
        end: usize,
    },
    RepeatBegin(usize),
    RepeatEnd {
        slot: usize,
        min: u64,
        head: usize,
    },
    Look {
        end: usize,
        negative: bool,
    },
    LookEnd,
    Accept,
}

enum Build {
    Node(NodeId, Modifiers),
    Emit(Instruction),
    Label(usize),
}

struct Budget(usize);
impl Budget {
    fn charge(&mut self, amount: usize) -> Result<(), MatchError> {
        self.0 = self
            .0
            .checked_sub(amount)
            .ok_or(MatchError::LimitExceeded)?;
        Ok(())
    }
}

type CaptureSpan = Option<(usize, usize)>;

fn merge_span(a: CaptureSpan, b: CaptureSpan) -> CaptureSpan {
    match (a, b) {
        (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
        (a, b) => a.or(b),
    }
}

/// Compile validated parser IR without recursive traversal. Lookbehind
/// and non-Unicode ignore-case matching are explicitly deferred.
/// The public arena is checked for invalid edges/indices before indexing it.
pub fn compile(pattern: &Pattern) -> Result<Program, MatchError> {
    let unicode = pattern.flags.unicode || pattern.flags.unicode_sets;
    let max_character = if unicode { 0x10ffff } else { 0xffff };
    if pattern.flags.ignore_case && !unicode {
        return Err(MatchError::Unsupported("ignore-case matching"));
    }
    if pattern.root.0 >= pattern.nodes.len() {
        return Err(MatchError::InvalidPattern);
    }
    let mut budget = Budget(MAX_MATCH_STEPS);
    budget.charge(pattern.nodes.len())?;
    budget.charge(pattern.captures.len())?;
    // Captures declared inside an atom occupy a contiguous interval in parser
    // opening-parenthesis order. Union spans are sufficient for iteration reset.
    let mut spans = Vec::with_capacity(pattern.nodes.len());
    for (index, node) in pattern.nodes.iter().enumerate() {
        budget.charge(1)?;
        let child = |id: NodeId| -> Result<CaptureSpan, MatchError> {
            if id.0 >= index {
                return Err(MatchError::InvalidPattern);
            }
            Ok(spans[id.0])
        };
        let valid_capture = |c: usize| c > 0 && c <= pattern.captures.len();
        let span = match node {
            Node::Alternative(ids) | Node::Disjunction(ids) => {
                if matches!(node, Node::Disjunction(_)) && ids.is_empty() {
                    return Err(MatchError::InvalidPattern);
                }
                let mut span = None;
                for &id in ids {
                    budget.charge(1)?;
                    span = merge_span(span, child(id)?);
                }
                span
            }
            Node::Group { body, capture } => {
                if capture.is_some_and(|c| !valid_capture(c)) {
                    return Err(MatchError::InvalidPattern);
                }
                merge_span(child(*body)?, capture.map(|c| (c, c)))
            }
            Node::Repeat { body, min, max, .. } => {
                if max.is_some_and(|max| max < *min) {
                    return Err(MatchError::InvalidPattern);
                }
                child(*body)?
            }
            Node::Lookaround { behind: true, .. } => {
                return Err(MatchError::Unsupported("lookbehind"));
            }
            Node::Lookaround { body, .. } => child(*body)?,
            Node::Modifiers {
                body,
                enable,
                disable,
            } => {
                if enable.ignore_case && !unicode {
                    return Err(MatchError::Unsupported("ignore-case matching"));
                }
                if enable.multiline && disable.multiline
                    || enable.dot_all && disable.dot_all
                    || enable.ignore_case && disable.ignore_case
                {
                    return Err(MatchError::InvalidPattern);
                }
                child(*body)?
            }
            Node::Literal(c) => {
                if *c > max_character {
                    return Err(MatchError::InvalidPattern);
                }
                None
            }
            Node::Backreference(ids) => {
                for &id in ids {
                    budget.charge(1)?;
                    if !valid_capture(id) {
                        return Err(MatchError::InvalidPattern);
                    }
                }
                None
            }
            Node::Class(class) => {
                for member in &class.members {
                    budget.charge(1)?;
                    if match member {
                        ClassMember::Literal(c) => *c > max_character,
                        ClassMember::Range(a, b) => a > b || *b > max_character,
                        ClassMember::Escape(_) => false,
                        ClassMember::Property { .. } => !pattern.flags.unicode,
                    } {
                        return Err(MatchError::InvalidPattern);
                    }
                }
                None
            }
            Node::Dot | Node::Assertion(_) => None,
        };
        spans.push(span);
    }
    let modifiers = Modifiers {
        multiline: pattern.flags.multiline,
        dot_all: pattern.flags.dot_all,
        ignore_case: pattern.flags.ignore_case,
    };
    let mut tasks = vec![
        Build::Emit(Instruction::Accept),
        Build::Node(pattern.root, modifiers),
    ];
    let mut code = Vec::new();
    let mut labels = Vec::new();
    let mut repeats = 0;
    while let Some(task) = tasks.pop() {
        budget.charge(1)?;
        match task {
            Build::Emit(instruction) => code.push(instruction),
            Build::Label(label) => labels[label] = code.len(),
            Build::Node(id, modifiers) => {
                let mut sequence = Vec::new();
                match &pattern.nodes[id.0] {
                    Node::Alternative(ids) => {
                        budget.charge(ids.len())?;
                        sequence.extend(ids.iter().map(|&id| Build::Node(id, modifiers)));
                    }
                    Node::Disjunction(ids) => {
                        budget.charge(ids.len())?;
                        let end = labels.len();
                        labels.push(0);
                        for (i, &id) in ids.iter().enumerate() {
                            if i + 1 == ids.len() {
                                sequence.push(Build::Node(id, modifiers));
                            } else {
                                let next = labels.len();
                                labels.push(0);
                                sequence.extend([
                                    Build::Emit(Instruction::Split(next)),
                                    Build::Node(id, modifiers),
                                    Build::Emit(Instruction::Jump(end)),
                                    Build::Label(next),
                                ]);
                            }
                        }
                        sequence.push(Build::Label(end));
                    }
                    Node::Group { body, capture } => {
                        if let Some(c) = capture {
                            sequence.push(Build::Emit(Instruction::Start(*c)));
                        }
                        sequence.push(Build::Node(*body, modifiers));
                        if let Some(c) = capture {
                            sequence.push(Build::Emit(Instruction::End(*c)));
                        }
                    }
                    Node::Modifiers {
                        body,
                        enable,
                        disable,
                    } => sequence.push(Build::Node(
                        *body,
                        Modifiers {
                            multiline: (modifiers.multiline || enable.multiline)
                                && !disable.multiline,
                            dot_all: (modifiers.dot_all || enable.dot_all) && !disable.dot_all,
                            ignore_case: (modifiers.ignore_case || enable.ignore_case)
                                && !disable.ignore_case,
                        },
                    )),
                    Node::Repeat {
                        body,
                        min,
                        max,
                        greedy,
                    } => {
                        let slot = repeats;
                        repeats += 1;
                        let head = labels.len();
                        let atom = head + 1;
                        let end = head + 2;
                        labels.extend([0; 3]);
                        sequence.extend([
                            Build::Emit(Instruction::RepeatInit(slot)),
                            Build::Label(head),
                            Build::Emit(Instruction::RepeatHead {
                                slot,
                                min: *min,
                                max: *max,
                                greedy: *greedy,
                                body: atom,
                                end,
                            }),
                            Build::Label(atom),
                            Build::Emit(Instruction::RepeatBegin(slot)),
                        ]);
                        if let Some((first, last)) = spans[body.0] {
                            sequence.push(Build::Emit(Instruction::Clear(first, last)));
                        }
                        sequence.extend([
                            Build::Node(*body, modifiers),
                            Build::Emit(Instruction::RepeatEnd {
                                slot,
                                min: *min,
                                head,
                            }),
                            Build::Label(end),
                        ]);
                    }
                    Node::Lookaround { body, negative, .. } => {
                        let end = labels.len();
                        labels.push(0);
                        sequence.extend([
                            Build::Emit(Instruction::Look {
                                end,
                                negative: *negative,
                            }),
                            Build::Node(*body, modifiers),
                            Build::Emit(Instruction::LookEnd),
                            Build::Label(end),
                        ]);
                    }
                    Node::Literal(c) => sequence.push(Build::Emit(Instruction::Char(
                        canonicalize(*c, modifiers.ignore_case),
                        modifiers.ignore_case,
                    ))),
                    Node::Dot => sequence.push(Build::Emit(Instruction::Dot(modifiers.dot_all))),
                    Node::Assertion(a) => sequence.push(Build::Emit(Instruction::Assert(
                        *a,
                        modifiers.multiline,
                        modifiers.ignore_case,
                    ))),
                    Node::Backreference(ids) => {
                        budget.charge(ids.len())?;
                        sequence.push(Build::Emit(Instruction::Backreference(
                            ids.clone(),
                            modifiers.ignore_case,
                        )));
                    }
                    Node::Class(class) => {
                        budget.charge(class.members.len())?;
                        let mut closed = class.clone();
                        if modifiers.ignore_case {
                            // Represent the image under Canonicalize. Keep ranges
                            // plus the folded targets of their nonidentity entries;
                            // runtime inputs are canonicalized before membership.
                            // Negation remains outside this operation.
                            for member in &mut closed.members {
                                if let ClassMember::Literal(cp) = member {
                                    *cp = canonicalize(*cp, true);
                                }
                            }
                            for member in &class.members {
                                if let ClassMember::Range(a, b) = member {
                                    let start = SIMPLE_FOLD.partition_point(|&(cp, _)| cp < *a);
                                    let end = SIMPLE_FOLD.partition_point(|&(cp, _)| cp <= *b);
                                    budget.charge(end - start)?;
                                    closed.members.extend(
                                        SIMPLE_FOLD[start..end]
                                            .iter()
                                            .map(|&(_, target)| ClassMember::Literal(target)),
                                    );
                                }
                            }
                        }
                        sequence.push(Build::Emit(Instruction::Class(
                            closed,
                            modifiers.ignore_case,
                        )));
                    }
                }
                tasks.extend(sequence.into_iter().rev());
            }
        }
    }
    for instruction in &mut code {
        budget.charge(1)?;
        match instruction {
            Instruction::Split(target)
            | Instruction::Jump(target)
            | Instruction::Look { end: target, .. }
            | Instruction::RepeatEnd { head: target, .. } => *target = labels[*target],
            Instruction::RepeatHead { body, end, .. } => {
                *body = labels[*body];
                *end = labels[*end];
            }
            _ => {}
        }
    }
    Ok(Program {
        code,
        captures: pattern.captures.len() + 1,
        repeats,
        sticky: pattern.flags.sticky,
        unicode,
    })
}

#[derive(Clone, Copy, Default)]
struct RepeatState {
    count: u64,
    start: usize,
}

enum Undo {
    Capture(usize, Option<Range<usize>>),
    Repeat(usize, RepeatState),
}

struct Backtrack {
    pc: usize,
    pos: usize,
    trail: usize,
    /// None is an ordinary choice; Some is an atomic assertion boundary.
    negative: Option<bool>,
}

struct Machine {
    pc: usize,
    pos: usize,
    captures: Vec<Option<Range<usize>>>,
    repeats: Vec<RepeatState>,
    trail: Vec<Undo>,
    backtrack: Vec<Backtrack>,
    budget: Budget,
}

impl Machine {
    fn capture(&mut self, index: usize, value: Option<Range<usize>>) -> Result<(), MatchError> {
        self.budget.charge(1)?;
        self.trail.push(Undo::Capture(
            index,
            std::mem::replace(&mut self.captures[index], value),
        ));
        Ok(())
    }
    fn repeat(&mut self, index: usize, value: RepeatState) -> Result<(), MatchError> {
        self.budget.charge(1)?;
        self.trail.push(Undo::Repeat(
            index,
            std::mem::replace(&mut self.repeats[index], value),
        ));
        Ok(())
    }
    fn rewind(&mut self, length: usize) -> Result<(), MatchError> {
        while self.trail.len() > length {
            self.budget.charge(1)?;
            match self.trail.pop().unwrap() {
                Undo::Capture(index, value) => self.captures[index] = value,
                Undo::Repeat(index, value) => self.repeats[index] = value,
            }
        }
        Ok(())
    }
    fn save(&mut self, pc: usize, negative: Option<bool>) -> Result<(), MatchError> {
        self.budget.charge(1)?;
        self.backtrack.push(Backtrack {
            pc,
            pos: self.pos,
            trail: self.trail.len(),
            negative,
        });
        Ok(())
    }
    fn fail(&mut self) -> Result<bool, MatchError> {
        while let Some(frame) = self.backtrack.pop() {
            self.budget.charge(1)?;
            self.rewind(frame.trail)?;
            self.pc = frame.pc;
            self.pos = frame.pos;
            if frame.negative != Some(false) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn look_end(&mut self) -> Result<bool, MatchError> {
        loop {
            self.budget.charge(1)?;
            let frame = self.backtrack.pop().expect("compiled lookahead boundary");
            if let Some(negative) = frame.negative {
                self.pc = frame.pc;
                self.pos = frame.pos;
                if negative {
                    self.rewind(frame.trail)?;
                }
                return Ok(!negative);
            }
        }
    }
    fn run(&mut self, program: &Program, input: &[u16]) -> Result<bool, MatchError> {
        loop {
            self.budget.charge(1)?;
            let instruction = &program.code[self.pc];
            self.pc += 1;
            let success = match instruction {
                Instruction::Accept => return Ok(true),
                Instruction::Char(c, ignore_case) => {
                    if let Some((actual, width)) = read_character(input, self.pos, program.unicode)
                        && canonicalize(actual, *ignore_case) == *c
                    {
                        self.pos += width;
                        true
                    } else {
                        false
                    }
                }
                Instruction::Dot(dot_all) => {
                    if let Some((c, width)) = read_character(input, self.pos, program.unicode)
                        && (*dot_all || !line(c))
                    {
                        self.pos += width;
                        true
                    } else {
                        false
                    }
                }
                Instruction::Class(class, ignore_case) => {
                    if let Some((c, width)) = read_character(input, self.pos, program.unicode) {
                        let c = canonicalize(c, *ignore_case);
                        let mut found = false;
                        for member in &class.members {
                            self.budget.charge(1)?;
                            found |= match member {
                                ClassMember::Literal(literal) => c == *literal,
                                ClassMember::Range(a, b) => (*a..=*b).contains(&c),
                                ClassMember::Escape(escape) => class_escape(*escape, c),
                                ClassMember::Property { property, inverted } => {
                                    property.contains(c, *inverted, *ignore_case)
                                }
                            };
                            if found {
                                break;
                            }
                        }
                        if found != class.inverted {
                            self.pos += width;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                Instruction::Assert(assertion, multiline, ignore_case) => {
                    // Line terminators and word characters are all BMP code
                    // points; either surrogate half has the same classification.
                    let previous = self
                        .pos
                        .checked_sub(1)
                        .and_then(|i| input.get(i))
                        .map(|&c| canonicalize(u32::from(c), *ignore_case));
                    let next = input
                        .get(self.pos)
                        .map(|&c| canonicalize(u32::from(c), *ignore_case));
                    match assertion {
                        Assertion::Start => {
                            self.pos == 0 || *multiline && previous.is_some_and(line)
                        }
                        Assertion::End => {
                            self.pos == input.len() || *multiline && next.is_some_and(line)
                        }
                        Assertion::WordBoundary => {
                            previous.is_some_and(word) != next.is_some_and(word)
                        }
                        Assertion::NotWordBoundary => {
                            previous.is_some_and(word) == next.is_some_and(word)
                        }
                    }
                }
                Instruction::Backreference(indices, ignore_case) => {
                    let mut range = None;
                    for &index in indices {
                        self.budget.charge(1)?;
                        if let Some(capture) = &self.captures[index] {
                            range = Some(capture.clone());
                            break;
                        }
                    }
                    let mut equal = true;
                    if let Some(range) = range {
                        let mut index = range.start;
                        while index < range.end {
                            self.budget.charge(1)?;
                            let (expected, width) = read_character(input, index, program.unicode)
                                .expect("capture is within input");
                            let Some((actual, actual_width)) =
                                read_character(input, self.pos, program.unicode)
                            else {
                                equal = false;
                                break;
                            };
                            if canonicalize(actual, *ignore_case)
                                != canonicalize(expected, *ignore_case)
                            {
                                equal = false;
                                break;
                            }
                            self.pos += actual_width;
                            index += width;
                        }
                    }
                    equal
                }
                Instruction::Start(index) => {
                    self.capture(*index, Some(self.pos..self.pos))?;
                    true
                }
                Instruction::End(index) => {
                    let start = self.captures[*index]
                        .as_ref()
                        .expect("compiled capture start")
                        .start;
                    self.capture(*index, Some(start..self.pos))?;
                    true
                }
                Instruction::Clear(first, last) => {
                    for index in *first..=*last {
                        self.capture(index, None)?;
                    }
                    true
                }
                Instruction::Split(target) => {
                    self.save(*target, None)?;
                    true
                }
                Instruction::Jump(target) => {
                    self.pc = *target;
                    true
                }
                Instruction::RepeatInit(slot) => {
                    self.repeat(*slot, RepeatState::default())?;
                    true
                }
                Instruction::RepeatHead {
                    slot,
                    min,
                    max,
                    greedy,
                    body,
                    end,
                } => {
                    let count = self.repeats[*slot].count;
                    if max.is_some_and(|max| count >= max) {
                        self.pc = *end;
                    } else if count < *min {
                        self.pc = *body;
                    } else if *greedy {
                        self.save(*end, None)?;
                        self.pc = *body;
                    } else {
                        self.save(*body, None)?;
                        self.pc = *end;
                    }
                    true
                }
                Instruction::RepeatBegin(slot) => {
                    self.repeat(
                        *slot,
                        RepeatState {
                            start: self.pos,
                            ..self.repeats[*slot]
                        },
                    )?;
                    true
                }
                Instruction::RepeatEnd { slot, min, head } => {
                    let state = self.repeats[*slot];
                    // RepeatMatcher rejects an empty iteration once the minimum
                    // was already satisfied. Failing here permits alternatives
                    // inside the atom to backtrack into a nonempty iteration.
                    if state.count >= *min && state.start == self.pos {
                        false
                    } else {
                        let count = state
                            .count
                            .checked_add(1)
                            .ok_or(MatchError::LimitExceeded)?;
                        self.repeat(*slot, RepeatState { count, ..state })?;
                        self.pc = *head;
                        true
                    }
                }
                Instruction::Look { end, negative } => {
                    self.save(*end, Some(*negative))?;
                    true
                }
                Instruction::LookEnd => self.look_end()?,
            };
            if !success && !self.fail()? {
                return Ok(false);
            }
        }
    }
}

/// Search from `start` (a UTF-16 offset), or only at `start` with the sticky flag.
/// Returns capture offsets, no match, or a distinct error. The single work limit
/// covers the entire search, not a fresh allowance per candidate or lookahead.
/// `g` and `d` do not change this core operation; lastIndex/objects belong to the
/// caller. `start > input.len()` gives no match.
/// Unicode mode maps a start inside a surrogate pair to its lead surrogate;
/// candidate positions and consuming instructions then advance by code point.
pub fn exec(
    program: &Program,
    input: &[u16],
    start: usize,
) -> Result<Option<Captures>, MatchError> {
    if start > input.len() {
        return Ok(None);
    }
    let start = if program.unicode
        && start > 0
        && matches!(input.get(start), Some(0xdc00..=0xdfff))
        && matches!(input.get(start - 1), Some(0xd800..=0xdbff))
    {
        start - 1
    } else {
        start
    };
    let mut budget = Budget(MAX_MATCH_STEPS);
    budget.charge(program.captures)?;
    budget.charge(program.repeats)?;
    let mut machine = Machine {
        pc: 0,
        pos: start,
        captures: vec![None; program.captures],
        repeats: vec![RepeatState::default(); program.repeats],
        trail: Vec::new(),
        backtrack: Vec::new(),
        budget,
    };
    let mut index = start;
    loop {
        machine.budget.charge(1)?;
        machine.pc = 0;
        machine.pos = index;
        if machine.run(program, input)? {
            machine.budget.charge(machine.captures.len())?;
            machine.captures[0] = Some(index..machine.pos);
            return Ok(Some(Captures {
                index,
                groups: machine.captures,
            }));
        }
        machine.rewind(0)?;
        if program.sticky || index == input.len() {
            break;
        }
        index = advance_string_index(input, index, program.unicode);
    }
    Ok(None)
}

/// Read a code unit, or a code point including an unpaired surrogate in
/// Unicode mode. All positions and widths remain UTF-16 code-unit offsets.
fn read_character(input: &[u16], index: usize, unicode: bool) -> Option<(u32, usize)> {
    let first = u32::from(*input.get(index)?);
    if unicode
        && (0xd800..=0xdbff).contains(&first)
        && let Some(&second @ 0xdc00..=0xdfff) = input.get(index + 1)
    {
        Some((
            0x10000 + ((first - 0xd800) << 10) + u32::from(second) - 0xdc00,
            2,
        ))
    } else {
        Some((first, 1))
    }
}

/// AdvanceStringIndex also advances one unit at the end for empty-match loops.
pub(crate) fn advance_string_index(input: &[u16], index: usize, unicode: bool) -> usize {
    index + read_character(input, index, unicode).map_or(1, |(_, width)| width)
}

fn line(c: u32) -> bool {
    matches!(c, 0x0a | 0x0d | 0x2028 | 0x2029)
}
fn word(c: u32) -> bool {
    matches!(c, 48..=57 | 65..=90 | 95 | 97..=122)
}
fn space(c: u32) -> bool {
    line(c)
        || matches!(
            c,
            0x09 | 0x0b | 0x0c | 0x20 | 0xa0 | 0x1680 | 0x2000
                ..=0x200a | 0x202f | 0x205f | 0x3000 | 0xfeff
        )
}
fn class_escape(escape: ClassEscape, c: u32) -> bool {
    match escape {
        ClassEscape::Digit => (48..=57).contains(&c),
        ClassEscape::NotDigit => !(48..=57).contains(&c),
        ClassEscape::Word => word(c),
        ClassEscape::NotWord => !word(c),
        ClassEscape::Space => space(c),
        ClassEscape::NotSpace => !space(c),
    }
}

#[cfg(test)]
mod tests;
