//! A small lexical subset for a future ClojureScript reader.
//!
//! Only parentheses, ASCII whitespace (09–0D and 20), and commas are special.
//! Everything else belongs to an atom, including Unicode whitespace, quotes,
//! semicolons, brackets, and reader prefixes. There is no string, comment, or
//! escape handling: even a parenthesis inside quotes is a delimiter. Matching
//! parentheses and classifying atoms belong to the later reader.

use std::{iter::FusedIterator, ops::Range};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    OpenParen,
    CloseParen,
    Atom,
}

/// A kind and a half-open byte span into the original source; no text is owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

/// An allocation-free byte scanner borrowing its UTF-8 source.
#[derive(Clone, Debug)]
pub struct Tokenizer<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }
}

const SEPARATOR: u8 = 1;
const PAREN: u8 = 2;

// A measured improvement over match-based classification on the benchmark
// fixtures. All non-ASCII bytes have class zero and stay inside their atom.
const CLASSES: [u8; 256] = {
    let mut table = [0; 256];
    let mut byte = b'\t';
    while byte <= b'\r' {
        table[byte as usize] = SEPARATOR;
        byte += 1;
    }
    table[b' ' as usize] = SEPARATOR;
    table[b',' as usize] = SEPARATOR;
    table[b'(' as usize] = PAREN;
    table[b')' as usize] = PAREN;
    table
};

#[inline]
fn is_separator(byte: u8) -> bool {
    CLASSES[byte as usize] & SEPARATOR != 0
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.source.as_bytes();
        while self.cursor < bytes.len() && is_separator(bytes[self.cursor]) {
            self.cursor += 1;
        }

        let start = self.cursor;
        let first = *bytes.get(start)?;
        self.cursor += 1;
        let kind = match first {
            b'(' => TokenKind::OpenParen,
            b')' => TokenKind::CloseParen,
            _ => {
                while self.cursor < bytes.len() {
                    let byte = bytes[self.cursor];
                    if CLASSES[byte as usize] != 0 {
                        break;
                    }
                    self.cursor += 1;
                }
                TokenKind::Atom
            }
        };

        // Boundaries occur only at ASCII delimiters or EOF. UTF-8 multibyte
        // encodings contain no ASCII bytes, so every span can slice source.
        Some(Token {
            kind,
            span: start..self.cursor,
        })
    }
}

impl FusedIterator for Tokenizer<'_> {}
