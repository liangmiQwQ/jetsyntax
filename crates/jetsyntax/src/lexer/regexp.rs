use std::ops::Range;

use super::{is_identifier_continue, is_identifier_start};

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Legacy,
    Unicode,
    UnicodeSets,
}

impl Mode {
    const fn unicode(self) -> bool {
        !matches!(self, Self::Legacy)
    }

    const fn unicode_sets(self) -> bool {
        matches!(self, Self::UnicodeSets)
    }
}

pub(super) fn validate_modifier_groups(pattern: &[u8]) -> Option<Range<usize>> {
    let mut index = 0;
    let mut in_class = false;
    while let Some(&byte) = pattern.get(index) {
        match byte {
            b'\\' => {
                // Escaped text cannot open a modifier group, even when its scalar is multibyte.
                index += 1;
                if let Some(&escaped) = pattern.get(index) {
                    index = (index + utf8_width(escaped)).min(pattern.len());
                }
            }
            b'[' if !in_class => {
                in_class = true;
                index += 1;
            }
            b']' if in_class => {
                in_class = false;
                index += 1;
            }
            b'(' if !in_class && pattern.get(index + 1) == Some(&b'?') => {
                if let Some(error) = validate_group_prefix(pattern, index + 1) {
                    return Some(error);
                }
                index += 2;
            }
            _ => index += utf8_width(byte),
        }
    }
    None
}

pub(super) fn validate_core_syntax(pattern: &[u8], mode: Mode) -> Option<Range<usize>> {
    let captures = match capture_summary(pattern, mode) {
        Ok(captures) => captures,
        Err(error) => return Some(error),
    };
    let mut index = 0;
    let mut target = None;
    let mut groups = GroupStack::new();

    while let Some(&byte) = pattern.get(index) {
        match byte {
            b'\\' => {
                let escaped = match validate_escape(pattern, index, mode, false, &captures) {
                    Ok(escaped) => escaped,
                    Err(error) => return Some(error),
                };
                target = Some(match escaped.kind {
                    EscapeKind::Assertion => QuantifierTarget::Assertion,
                    EscapeKind::Singleton(_) | EscapeKind::Set => QuantifierTarget::Atom,
                });
                index = escaped.end;
            }
            b'[' => {
                target = Some(QuantifierTarget::Atom);
                index = if matches!(mode, Mode::Unicode) {
                    match validate_unicode_class(pattern, index, &captures) {
                        Ok(end) => end,
                        Err(error) => return Some(error),
                    }
                } else {
                    class_end(pattern, index, mode)
                };
            }
            b'(' => {
                let (kind, content) = group_prefix(pattern, index);
                groups.push(kind);
                target = None;
                index = content;
            }
            b')' => {
                let Some(kind) = groups.pop() else {
                    return Some(index..index + 1);
                };
                target = Some(kind.target());
                index += 1;
            }
            b'|' => {
                target = None;
                index += 1;
            }
            b'^' | b'$' => {
                target = Some(QuantifierTarget::Assertion);
                index += 1;
            }
            b'*' | b'+' | b'?' => {
                let end = index + 1;
                if !target.is_some_and(|target| target.quantifiable(mode)) {
                    return Some(index..end);
                }
                index = lazy_quantifier_end(pattern, end);
                target = Some(QuantifierTarget::Quantified);
            }
            b'{' => {
                if let Some((end, ordered)) = braced_quantifier(pattern, index) {
                    if !ordered || !target.is_some_and(|target| target.quantifiable(mode)) {
                        return Some(index..end);
                    }
                    index = lazy_quantifier_end(pattern, end);
                    target = Some(QuantifierTarget::Quantified);
                } else if mode.unicode() {
                    return Some(index..index + 1);
                } else {
                    target = Some(QuantifierTarget::Atom);
                    index += 1;
                }
            }
            b'}' | b']' if mode.unicode() => return Some(index..index + 1),
            _ => {
                target = Some(QuantifierTarget::Atom);
                index += utf8_width(byte);
            }
        }
    }

    (!groups.is_empty()).then_some(pattern.len().saturating_sub(1)..pattern.len())
}

#[derive(Default)]
struct CaptureSummary {
    count: usize,
    names: Vec<Range<usize>>,
}

fn capture_summary(pattern: &[u8], mode: Mode) -> Result<CaptureSummary, Range<usize>> {
    let mut summary = CaptureSummary::default();
    let mut index = 0;
    while let Some(&byte) = pattern.get(index) {
        match byte {
            // Only the escaped character is opaque here. Consuming a malformed `\k<...` payload
            // could hide a later named capture that changes legacy reference grammar.
            b'\\' => {
                index += 1;
                if let Some(&escaped) = pattern.get(index) {
                    index = (index + utf8_width(escaped)).min(pattern.len());
                }
            }
            b'[' => index = class_end(pattern, index, mode),
            b'(' if pattern.get(index + 1) != Some(&b'?') => {
                summary.count += 1;
                index += 1;
            }
            b'(' if pattern.get(index + 2) == Some(&b'<')
                && !matches!(pattern.get(index + 3), Some(b'=' | b'!')) =>
            {
                summary.count += 1;
                let (name, end) = group_name(pattern, index + 3)?;
                summary.names.push(name);
                index = end;
            }
            _ => index += utf8_width(byte),
        }
    }
    Ok(summary)
}

#[derive(Clone, Copy)]
struct Escape {
    end: usize,
    kind: EscapeKind,
}

#[derive(Clone, Copy)]
enum EscapeKind {
    Assertion,
    Singleton(u32),
    Set,
}

fn validate_escape(
    pattern: &[u8],
    start: usize,
    mode: Mode,
    in_class: bool,
    captures: &CaptureSummary,
) -> Result<Escape, Range<usize>> {
    let escaped = pattern.get(start + 1).copied();
    let payload = start + 2;
    if !mode.unicode() {
        if escaped == Some(b'k') && !captures.names.is_empty() {
            if in_class || pattern.get(payload) != Some(&b'<') {
                return Err(start..payload.min(pattern.len()));
            }
            let end = validate_named_reference(pattern, start, payload + 1, captures)?;
            return Ok(Escape {
                end,
                kind: EscapeKind::Singleton(0),
            });
        }
        return Ok(Escape {
            end: escape_end(pattern, start, mode),
            kind: if !in_class && matches!(escaped, Some(b'b' | b'B')) {
                EscapeKind::Assertion
            } else if matches!(escaped, Some(b'd' | b'D' | b's' | b'S' | b'w' | b'W')) {
                EscapeKind::Set
            } else {
                EscapeKind::Singleton(escaped.unwrap_or_default().into())
            },
        });
    }

    let Some(escaped) = escaped else {
        return Err(start..pattern.len());
    };
    let invalid = || start..(payload + utf8_width(escaped)).min(pattern.len());
    let singleton = |end, value| {
        Ok(Escape {
            end,
            kind: EscapeKind::Singleton(value),
        })
    };

    match escaped {
        b'b' | b'B' if !in_class => Ok(Escape {
            end: payload,
            kind: EscapeKind::Assertion,
        }),
        b'b' => singleton(payload, 0x08),
        b'd' | b'D' | b's' | b'S' | b'w' | b'W' => Ok(Escape {
            end: payload,
            kind: EscapeKind::Set,
        }),
        b'f' => singleton(payload, 0x0C),
        b'n' => singleton(payload, 0x0A),
        b'r' => singleton(payload, 0x0D),
        b't' => singleton(payload, 0x09),
        b'v' => singleton(payload, 0x0B),
        b'c' if pattern.get(payload).is_some_and(u8::is_ascii_alphabetic) => {
            singleton(payload + 1, u32::from(pattern[payload] & 0x1F))
        }
        b'0' if !pattern.get(payload).is_some_and(u8::is_ascii_digit) => singleton(payload, 0),
        b'1'..=b'9' if !in_class => {
            let end = decimal_end(pattern, start + 1);
            if decimal_reference_within(&pattern[start + 1..end], captures.count) {
                singleton(end, 0)
            } else {
                Err(start..end)
            }
        }
        b'x' if has_hex_digits(pattern, payload, 2) => {
            singleton(payload + 2, hex_value(&pattern[payload..payload + 2]))
        }
        b'u' if pattern.get(payload) == Some(&b'{') => {
            let (end, value) = braced_unicode_escape(pattern, start)?;
            singleton(end, value)
        }
        b'u' if has_hex_digits(pattern, payload, 4) => {
            singleton(payload + 4, hex_value(&pattern[payload..payload + 4]))
        }
        b'p' | b'P' if pattern.get(payload) == Some(&b'{') => {
            let Some(end) = delimited_end(pattern, payload + 1, b'}') else {
                return Err(start..pattern.len());
            };
            if end == payload + 2 {
                return Err(start..end);
            }
            Ok(Escape {
                end,
                kind: EscapeKind::Set,
            })
        }
        b'k' if !in_class && !captures.names.is_empty() && pattern.get(payload) == Some(&b'<') => {
            let end = validate_named_reference(pattern, start, payload + 1, captures)?;
            singleton(end, 0)
        }
        byte if is_syntax_character(byte) || byte == b'/' || in_class && byte == b'-' => {
            singleton(payload, byte.into())
        }
        _ => Err(invalid()),
    }
}

fn validate_unicode_class(
    pattern: &[u8],
    start: usize,
    captures: &CaptureSummary,
) -> Result<usize, Range<usize>> {
    let mut index = start + 1;
    if pattern.get(index) == Some(&b'^') {
        index += 1;
    }

    while let Some(&byte) = pattern.get(index) {
        if byte == b']' {
            return Ok(index + 1);
        }

        let left = unicode_class_atom(pattern, index, captures)?;
        index = left.end;
        if pattern.get(index) != Some(&b'-') || pattern.get(index + 1) == Some(&b']') {
            continue;
        }

        let right = unicode_class_atom(pattern, index + 1, captures)?;
        if matches!(left.kind, EscapeKind::Set) || matches!(right.kind, EscapeKind::Set) {
            return Err(left.start..right.end);
        }
        if let (EscapeKind::Singleton(left_value), EscapeKind::Singleton(right_value)) =
            (left.kind, right.kind)
            && left_value > right_value
        {
            return Err(left.start..right.end);
        }
        index = right.end;
    }

    Err(start..pattern.len())
}

#[derive(Clone, Copy)]
struct ClassAtom {
    start: usize,
    end: usize,
    kind: EscapeKind,
}

fn unicode_class_atom(
    pattern: &[u8],
    start: usize,
    captures: &CaptureSummary,
) -> Result<ClassAtom, Range<usize>> {
    let Some(&byte) = pattern.get(start) else {
        return Err(start..pattern.len());
    };
    if byte == b']' {
        return Err(start..start + 1);
    }
    if byte == b'\\' {
        let escape = validate_escape(pattern, start, Mode::Unicode, true, captures)?;
        if let EscapeKind::Singleton(lead) = escape.kind
            && (0xD800..=0xDBFF).contains(&lead)
            && pattern.get(escape.end..escape.end + 2) == Some(b"\\u")
            && has_hex_digits(pattern, escape.end + 2, 4)
        {
            let trail = hex_value(&pattern[escape.end + 2..escape.end + 6]);
            if (0xDC00..=0xDFFF).contains(&trail) {
                return Ok(ClassAtom {
                    start,
                    end: escape.end + 6,
                    kind: EscapeKind::Singleton(
                        0x1_0000 + ((lead - 0xD800) << 10) + trail - 0xDC00,
                    ),
                });
            }
        }
        return Ok(ClassAtom {
            start,
            end: escape.end,
            kind: escape.kind,
        });
    }

    let (value, width) = scalar_value(pattern, start);
    Ok(ClassAtom {
        start,
        end: (start + width).min(pattern.len()),
        kind: EscapeKind::Singleton(value),
    })
}

fn group_name(pattern: &[u8], start: usize) -> Result<(Range<usize>, usize), Range<usize>> {
    let mut index = start;
    let mut first = true;
    while let Some(&byte) = pattern.get(index) {
        if byte == b'>' {
            return if first {
                Err(start..index + 1)
            } else {
                Ok((start..index, index + 1))
            };
        }

        let character_start = index;
        let (character, end) = group_name_character(pattern, index)?;
        let valid = if first {
            is_identifier_start(character)
        } else {
            is_identifier_continue(character)
        };
        if !valid {
            return Err(character_start..end);
        }
        first = false;
        index = end;
    }
    Err(start.saturating_sub(2)..pattern.len())
}

fn group_name_character(pattern: &[u8], start: usize) -> Result<(char, usize), Range<usize>> {
    if pattern.get(start) != Some(&b'\\') {
        let (value, width) = scalar_value(pattern, start);
        let Some(character) = char::from_u32(value) else {
            return Err(start..(start + width).min(pattern.len()));
        };
        return Ok((character, start + width));
    }

    if pattern.get(start + 1) != Some(&b'u') {
        return Err(start..(start + 2).min(pattern.len()));
    }
    if pattern.get(start + 2) == Some(&b'{') {
        let (end, value) = braced_unicode_escape(pattern, start)?;
        let Some(character) = char::from_u32(value) else {
            return Err(start..end);
        };
        return Ok((character, end));
    }
    if !has_hex_digits(pattern, start + 2, 4) {
        return Err(start..(start + 6).min(pattern.len()));
    }

    let lead = hex_value(&pattern[start + 2..start + 6]);
    let mut end = start + 6;
    let value = if (0xD800..=0xDBFF).contains(&lead)
        && pattern.get(end..end + 2) == Some(b"\\u")
        && has_hex_digits(pattern, end + 2, 4)
    {
        let trail = hex_value(&pattern[end + 2..end + 6]);
        if (0xDC00..=0xDFFF).contains(&trail) {
            end += 6;
            0x1_0000 + ((lead - 0xD800) << 10) + trail - 0xDC00
        } else {
            lead
        }
    } else {
        lead
    };
    let Some(character) = char::from_u32(value) else {
        return Err(start..end);
    };
    Ok((character, end))
}

fn validate_named_reference(
    pattern: &[u8],
    start: usize,
    name_start: usize,
    captures: &CaptureSummary,
) -> Result<usize, Range<usize>> {
    let (reference, end) = group_name(pattern, name_start)?;
    if captures
        .names
        .iter()
        .any(|capture| group_names_equal(pattern, capture.clone(), reference.clone()))
    {
        Ok(end)
    } else {
        Err(start..end)
    }
}

fn group_names_equal(pattern: &[u8], left: Range<usize>, right: Range<usize>) -> bool {
    let mut left_index = left.start;
    let mut right_index = right.start;
    while left_index < left.end && right_index < right.end {
        let Ok((left_character, left_end)) = group_name_character(pattern, left_index) else {
            return false;
        };
        let Ok((right_character, right_end)) = group_name_character(pattern, right_index) else {
            return false;
        };
        if left_character != right_character {
            return false;
        }
        left_index = left_end;
        right_index = right_end;
    }
    left_index == left.end && right_index == right.end
}

fn braced_unicode_escape(pattern: &[u8], start: usize) -> Result<(usize, u32), Range<usize>> {
    let mut index = start + 3;
    let digits = index;
    let mut value = 0_u32;
    while let Some(&byte) = pattern.get(index) {
        if byte == b'}' {
            return if index == digits || value > 0x10_FFFF {
                Err(start..index + 1)
            } else {
                Ok((index + 1, value))
            };
        }
        let Some(digit) = hex_digit_value(byte) else {
            return Err(start..(index + utf8_width(byte)).min(pattern.len()));
        };
        value = value.saturating_mul(16).saturating_add(digit);
        index += 1;
    }
    Err(start..pattern.len())
}

fn decimal_reference_within(digits: &[u8], captures: usize) -> bool {
    let mut value = 0_usize;
    for &digit in digits {
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(digit - b'0'));
        if value > captures {
            return false;
        }
    }
    value != 0
}

fn hex_value(digits: &[u8]) -> u32 {
    digits.iter().fold(0, |value, byte| {
        value * 16 + hex_digit_value(*byte).unwrap_or(0)
    })
}

const fn hex_digit_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
        b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}

const fn is_syntax_character(byte: u8) -> bool {
    matches!(
        byte,
        b'^' | b'$'
            | b'\\'
            | b'.'
            | b'*'
            | b'+'
            | b'?'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'|'
    )
}

fn scalar_value(pattern: &[u8], index: usize) -> (u32, usize) {
    let width = utf8_width(pattern[index]).min(pattern.len() - index);
    let value = std::str::from_utf8(&pattern[index..index + width])
        .ok()
        .and_then(|text| text.chars().next())
        .map_or_else(|| u32::from(pattern[index]), u32::from);
    (value, width)
}

#[derive(Clone, Copy)]
enum QuantifierTarget {
    Atom,
    LookAhead,
    LookBehind,
    Assertion,
    Quantified,
}

impl QuantifierTarget {
    const fn quantifiable(self, mode: Mode) -> bool {
        matches!(self, Self::Atom) || matches!(self, Self::LookAhead) && !mode.unicode()
    }
}

#[derive(Clone, Copy)]
enum GroupKind {
    Atom,
    LookAhead,
    LookBehind,
}

impl GroupKind {
    const fn target(self) -> QuantifierTarget {
        match self {
            Self::Atom => QuantifierTarget::Atom,
            Self::LookAhead => QuantifierTarget::LookAhead,
            Self::LookBehind => QuantifierTarget::LookBehind,
        }
    }
}

const INLINE_GROUP_DEPTH: usize = 64;

struct GroupStack {
    inline: [GroupKind; INLINE_GROUP_DEPTH],
    depth: usize,
    overflow: Vec<GroupKind>,
}

impl GroupStack {
    const fn new() -> Self {
        Self {
            inline: [GroupKind::Atom; INLINE_GROUP_DEPTH],
            depth: 0,
            overflow: Vec::new(),
        }
    }

    fn push(&mut self, kind: GroupKind) {
        if self.depth < INLINE_GROUP_DEPTH {
            self.inline[self.depth] = kind;
        } else {
            // Ordinary source stays allocation-free; adversarial depth remains linear and correct.
            self.overflow.push(kind);
        }
        self.depth += 1;
    }

    fn pop(&mut self) -> Option<GroupKind> {
        self.depth = self.depth.checked_sub(1)?;
        if self.depth < INLINE_GROUP_DEPTH {
            Some(self.inline[self.depth])
        } else {
            self.overflow.pop()
        }
    }

    const fn is_empty(&self) -> bool {
        self.depth == 0
    }
}

fn group_prefix(pattern: &[u8], start: usize) -> (GroupKind, usize) {
    if pattern.get(start + 1) != Some(&b'?') {
        return (GroupKind::Atom, start + 1);
    }

    match pattern.get(start + 2).copied() {
        Some(b'=' | b'!') => (GroupKind::LookAhead, start + 3),
        Some(b'<') if matches!(pattern.get(start + 3), Some(b'=' | b'!')) => {
            (GroupKind::LookBehind, start + 4)
        }
        Some(b'<') => (
            GroupKind::Atom,
            delimited_end(pattern, start + 3, b'>').unwrap_or(start + 3),
        ),
        Some(b':') => (GroupKind::Atom, start + 3),
        Some(b'i' | b'm' | b's' | b'-') => (
            GroupKind::Atom,
            delimited_end(pattern, start + 2, b':').unwrap_or(start + 2),
        ),
        Some(_) => (GroupKind::Atom, start + 2),
        None => (GroupKind::Atom, pattern.len()),
    }
}

fn escape_end(pattern: &[u8], start: usize, mode: Mode) -> usize {
    let Some(&escaped) = pattern.get(start + 1) else {
        return pattern.len();
    };
    let payload = start + 2;

    match escaped {
        b'u' | b'p' | b'P' if mode.unicode() && pattern.get(payload) == Some(&b'{') => {
            delimited_end(pattern, payload + 1, b'}').unwrap_or(pattern.len())
        }
        b'k' if pattern.get(payload) == Some(&b'<') => {
            delimited_end(pattern, payload + 1, b'>').unwrap_or(pattern.len())
        }
        b'u' if has_hex_digits(pattern, payload, 4) => payload + 4,
        b'x' if has_hex_digits(pattern, payload, 2) => payload + 2,
        b'c' => pattern
            .get(payload)
            .map_or(payload, |byte| payload + utf8_width(*byte)),
        b'0'..=b'9' => {
            let mut end = payload;
            while pattern.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            end
        }
        _ => (start + 1 + utf8_width(escaped)).min(pattern.len()),
    }
}

fn class_end(pattern: &[u8], start: usize, mode: Mode) -> usize {
    let mut index = start + 1;
    let mut depth = 1;
    while let Some(&byte) = pattern.get(index) {
        match byte {
            b'\\' => index = escape_end(pattern, index, mode),
            b'[' if mode.unicode_sets() => {
                depth += 1;
                index += 1;
            }
            b']' => {
                depth -= 1;
                index += 1;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += utf8_width(byte),
        }
    }
    pattern.len()
}

fn braced_quantifier(pattern: &[u8], start: usize) -> Option<(usize, bool)> {
    let minimum_start = start + 1;
    let minimum_end = decimal_end(pattern, minimum_start);
    if minimum_end == minimum_start {
        return None;
    }
    if pattern.get(minimum_end) == Some(&b'}') {
        return Some((minimum_end + 1, true));
    }
    if pattern.get(minimum_end) != Some(&b',') {
        return None;
    }

    let maximum_start = minimum_end + 1;
    let maximum_end = decimal_end(pattern, maximum_start);
    if pattern.get(maximum_end) != Some(&b'}') {
        return None;
    }
    let ordered = maximum_start == maximum_end
        || decimal_less_than_or_equal(
            &pattern[minimum_start..minimum_end],
            &pattern[maximum_start..maximum_end],
        );
    Some((maximum_end + 1, ordered))
}

fn decimal_end(pattern: &[u8], mut index: usize) -> usize {
    while pattern.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    index
}

fn decimal_less_than_or_equal(left: &[u8], right: &[u8]) -> bool {
    let left = trim_decimal_zeroes(left);
    let right = trim_decimal_zeroes(right);
    left.len() < right.len() || left.len() == right.len() && left <= right
}

fn trim_decimal_zeroes(mut value: &[u8]) -> &[u8] {
    while value.first() == Some(&b'0') {
        value = &value[1..];
    }
    value
}

fn lazy_quantifier_end(pattern: &[u8], end: usize) -> usize {
    end + usize::from(pattern.get(end) == Some(&b'?'))
}

fn delimited_end(pattern: &[u8], mut index: usize, delimiter: u8) -> Option<usize> {
    while let Some(&byte) = pattern.get(index) {
        if byte == delimiter {
            return Some(index + 1);
        }
        index += utf8_width(byte);
    }
    None
}

fn has_hex_digits(pattern: &[u8], start: usize, length: usize) -> bool {
    pattern
        .get(start..start + length)
        .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
}

fn validate_group_prefix(pattern: &[u8], question: usize) -> Option<Range<usize>> {
    let prefix = question + 1;
    match pattern.get(prefix).copied() {
        Some(b':' | b'=' | b'!' | b'<') => None,
        Some(b'i' | b'm' | b's' | b'-') => validate_modifier_list(pattern, prefix),
        Some(byte) => Some(prefix..(prefix + utf8_width(byte)).min(pattern.len())),
        None => Some(question..question + 1),
    }
}

fn validate_modifier_list(pattern: &[u8], start: usize) -> Option<Range<usize>> {
    // Modifier names are literal ASCII tokens; identifier escapes are not accepted here.
    let mut index = start;
    let mut adding = 0_u8;
    let mut removing = 0_u8;
    let mut remove_list = false;
    while let Some(&byte) = pattern.get(index) {
        let bit = match byte {
            b'i' => 1 << 0,
            b'm' => 1 << 1,
            b's' => 1 << 2,
            b'-' if !remove_list => {
                remove_list = true;
                index += 1;
                continue;
            }
            b':' => {
                return ((adding | removing) == 0).then_some(start..index + 1);
            }
            _ => return Some(index..(index + utf8_width(byte)).min(pattern.len())),
        };
        if (adding | removing) & bit != 0 {
            return Some(index..index + 1);
        }
        if remove_list {
            removing |= bit;
        } else {
            adding |= bit;
        }
        index += 1;
    }
    Some(start..pattern.len())
}

const fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}
