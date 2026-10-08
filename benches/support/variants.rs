//! The original match-based scanner and a compact-span experiment, kept out
//! of the public library. The frozen subset scanner uses the measured table classifier.

use std::ops::Range;

use super::subset::{Token, TokenKind};

#[inline]
fn separator(byte: u8) -> bool {
    matches!(byte, b'\t'..=b'\r' | b' ' | b',')
}

#[inline]
fn next_span(source: &str, cursor: &mut usize) -> Option<(TokenKind, Range<usize>)> {
    let bytes = source.as_bytes();
    while *cursor < bytes.len() && separator(bytes[*cursor]) {
        *cursor += 1;
    }
    let start = *cursor;
    let first = *bytes.get(start)?;
    *cursor += 1;
    let kind = match first {
        b'(' => TokenKind::OpenParen,
        b')' => TokenKind::CloseParen,
        _ => {
            while *cursor < bytes.len() {
                let byte = bytes[*cursor];
                if separator(byte) || matches!(byte, b'(' | b')') {
                    break;
                }
                *cursor += 1;
            }
            TokenKind::Atom
        }
    };
    Some((kind, start..*cursor))
}

pub struct MatchTokenizer<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> MatchTokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }
}

impl Iterator for MatchTokenizer<'_> {
    type Item = Token;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        next_span(self.source, &mut self.cursor).map(|(kind, span)| Token { kind, span })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct CompactToken {
    pub kind: TokenKind,
    pub span: Range<u32>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SourceTooLarge;

pub struct CompactTokenizer<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> CompactTokenizer<'a> {
    // Kept separately to exercise the limit without allocating a >4 GiB input.
    pub fn validate_len(len: usize) -> Result<(), SourceTooLarge> {
        u32::try_from(len).map(|_| ()).map_err(|_| SourceTooLarge)
    }

    pub fn new(source: &'a str) -> Result<Self, SourceTooLarge> {
        Self::validate_len(source.len())?;
        Ok(Self { source, cursor: 0 })
    }
}

impl Iterator for CompactTokenizer<'_> {
    type Item = CompactToken;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        next_span(self.source, &mut self.cursor).map(|(kind, span)| CompactToken {
            kind,
            // new() validates the entire source before any narrowing; cursor
            // never exceeds source.len(), including the exclusive end at EOF.
            span: span.start as u32..span.end as u32,
        })
    }
}
