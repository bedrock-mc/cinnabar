//! Acceptance follows the pinned Go x/text parser; canonical output never rewrites signed metadata.
//! Source-attributed adaptation of Go's BSD-licensed internal/language/parse.go; see notices.

use super::locale_generated::{
    BASE_THREE, BASE_TWO, GRANDFATHERED, REGION_NUMBER, REGION_TWO, SCRIPTS, VARIANTS,
};
use std::{collections::BTreeMap, str::Split};

/// Preserves registry admission, aliases, underscore handling and Go's extension-prefix behavior.
pub(super) fn valid(input: &str) -> bool {
    let mut input = input.replace('_', "-");
    input.make_ascii_lowercase();
    if GRANDFATHERED.binary_search(&input.as_str()).is_ok() {
        return true;
    }
    let mut cursor = Cursor::new(&input);
    let Some(first) = cursor.current else {
        return false;
    };
    if first.word == "x" {
        return extensions(&mut cursor) && !cursor.invalid;
    }
    if !tag(&mut cursor, true) {
        return false;
    }
    if cursor.current.is_some_and(|token| token.word.len() == 1) {
        // Go's extension parser returns its valid prefix without rejecting an unconsumed tail.
        extensions(&mut cursor) && !cursor.invalid
    } else {
        cursor.current.is_none() && !cursor.invalid
    }
}

/// Consumes the actual base/extlang/script/region/variant phases; transform extlangs are not checked.
fn tag(cursor: &mut Cursor<'_>, normalize_extlang: bool) -> bool {
    let Some(first) = cursor.current else {
        return false;
    };
    if !base(first.word) {
        return false;
    }
    cursor.advance();
    while let Some(token) = cursor.current {
        if token.word.len() != 3 || !token.word.as_bytes()[0].is_ascii_alphabetic() {
            break;
        }
        if normalize_extlang && !base(token.word) {
            return false;
        }
        cursor.advance();
    }
    if let Some(token) = cursor.current
        && token.word.len() == 4
        && token.word.as_bytes()[0].is_ascii_alphabetic()
    {
        if SCRIPTS.binary_search(&token.word).is_err() {
            return false;
        }
        cursor.advance();
    }
    if let Some(token) = cursor.current
        && (2..=3).contains(&token.word.len())
    {
        if !region(token.word) {
            return false;
        }
        cursor.advance();
    }
    let mut last = None;
    let mut count = 0;
    let mut sorting = false;
    while let Some(token) = cursor.current {
        if token.word.len() < 4 {
            break;
        }
        let Ok(index) = VARIANTS.binary_search_by_key(&token.word, |row| row.0) else {
            return false;
        };
        let id = VARIANTS[index].1;
        count += 1;
        if !sorting {
            if last.is_some_and(|previous| previous >= id) {
                sorting = true;
                // The source only limits the first out-of-order occurrence, then de-duplicates.
                if count > 8 {
                    return false;
                }
            } else {
                last = Some(id);
            }
        }
        cursor.advance();
    }
    !cursor.invalid
}

/// Singleton extensions share the source parser's minimum lengths; private use consumes all tokens.
fn extensions(cursor: &mut Cursor<'_>) -> bool {
    while let Some(singleton) = cursor.current {
        if singleton.word.len() != 1 {
            break;
        }
        cursor.advance();
        let end = match singleton.word {
            "u" => unicode(cursor),
            "t" => transformed(cursor),
            "x" => cursor.consume(1),
            _ => cursor.consume(2),
        };
        let Some(end) = end else {
            return false;
        };
        let minimum = if singleton.word == "x" { 3 } else { 4 };
        if end.saturating_sub(singleton.start) < minimum || cursor.invalid {
            return false;
        }
        if singleton.word == "x" {
            break;
        }
    }
    !cursor.invalid
}

/// Attributes may repeat; identical repeated keys pass, while different types for one key fail.
fn unicode(cursor: &mut Cursor<'_>) -> Option<usize> {
    let mut end = cursor.consume(3);
    let input = cursor.input;
    let mut keys = BTreeMap::new();
    while let Some(key) = cursor.current {
        if key.word.len() != 2 {
            break;
        }
        cursor.advance();
        let key_end = cursor.consume(3).unwrap_or(key.end);
        let value = &input[key.start..key_end];
        if keys.insert(key.word, value).is_some_and(|old| old != value) {
            return None;
        }
        end = Some(key_end);
    }
    end
}

/// Go accepts a transform language or its digit-ended keys, including a key without type subtags.
fn transformed(cursor: &mut Cursor<'_>) -> Option<usize> {
    let mut end = None;
    if let Some(token) = cursor.current
        && (2..=3).contains(&token.word.len())
        && token.word.as_bytes()[1].is_ascii_alphabetic()
    {
        if !tag(cursor, false) {
            return None;
        }
        end = cursor.previous_end;
    }
    while let Some(key) = cursor.current {
        if key.word.len() != 2 || key.word.as_bytes()[1].is_ascii_alphabetic() {
            break;
        }
        cursor.advance();
        end = Some(cursor.consume(3).unwrap_or(key.end));
    }
    end
}

/// Checks the actual finite two/three-letter membership domain rather than a generic BCP47 grammar.
fn base(word: &str) -> bool {
    match word.len() {
        2 => member(word, b'a', 26, BASE_TWO),
        3 => member(word, b'a', 26, BASE_THREE),
        _ => false,
    }
}

/// Region tokens at this phase are alpha-two or numeric-three; alpha-three was an extlang above.
fn region(word: &str) -> bool {
    match word.len() {
        2 => member(word, b'a', 26, REGION_TWO),
        3 => member(word, b'0', 10, REGION_NUMBER),
        _ => false,
    }
}

/// Indexes only the generated alphabet domain; malformed bytes cannot address outside its bitset.
fn member(word: &str, first: u8, radix: usize, bits: &[u8]) -> bool {
    let mut index = 0usize;
    for byte in word.bytes() {
        let Some(digit) = byte
            .checked_sub(first)
            .filter(|digit| usize::from(*digit) < radix)
        else {
            return false;
        };
        index = index * radix + usize::from(digit);
    }
    bits.get(index / 8)
        .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
}

#[derive(Clone, Copy)]
struct Token<'a> {
    word: &'a str,
    start: usize,
    end: usize,
}

struct Cursor<'a> {
    input: &'a str,
    remaining: Split<'a, char>,
    offset: usize,
    current: Option<Token<'a>>,
    previous_end: Option<usize>,
    invalid: bool,
}

impl<'a> Cursor<'a> {
    /// Scans lazily: the source parser need not inspect data beyond an accepted extension prefix.
    fn new(input: &'a str) -> Self {
        let mut cursor = Self {
            input,
            remaining: input.split('-'),
            offset: 0,
            current: None,
            previous_end: None,
            invalid: false,
        };
        cursor.advance();
        cursor
    }

    /// Any malformed token actually reached makes admission fail; no second JSON/string parser exists.
    fn advance(&mut self) {
        if let Some(token) = self.current.take() {
            self.previous_end = Some(token.end);
        }
        let Some(word) = self.remaining.next() else {
            return;
        };
        let start = self.offset;
        self.offset = start + word.len() + 1;
        if word.is_empty()
            || word.len() > 8
            || !word.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            self.invalid = true;
            return;
        }
        self.current = Some(Token {
            word,
            start,
            end: start + word.len(),
        });
    }

    /// Consumes only subtags in one extension's allowed size class, retaining its first next token.
    fn consume(&mut self, minimum: usize) -> Option<usize> {
        let mut end = None;
        while let Some(token) = self.current {
            if token.word.len() < minimum {
                break;
            }
            end = Some(token.end);
            self.advance();
        }
        end
    }
}
