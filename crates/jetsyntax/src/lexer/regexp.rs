use std::ops::Range;

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
