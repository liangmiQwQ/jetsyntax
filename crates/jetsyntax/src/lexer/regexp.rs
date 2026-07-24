use std::ops::Range;

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
    let mut index = 0;
    let mut target = None;
    let mut groups = GroupStack::new();

    while let Some(&byte) = pattern.get(index) {
        match byte {
            b'\\' => {
                let escaped = pattern.get(index + 1).copied();
                target = Some(if matches!(escaped, Some(b'b' | b'B')) {
                    QuantifierTarget::Assertion
                } else {
                    QuantifierTarget::Atom
                });
                index = escape_end(pattern, index, mode);
            }
            b'[' => {
                target = Some(QuantifierTarget::Atom);
                index = class_end(pattern, index, mode);
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
