//! Text checks shared by the caption, alignment and pacing rules.

/// Upper bound for any single text field, in UTF-8 bytes. Longer fields are rejected, never truncated.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

pub fn within_limit(text: &str) -> bool {
    text.len() <= MAX_TEXT_BYTES
}

/// True when the text has at least one character that is neither whitespace nor punctuation.
pub fn is_meaningful(text: &str) -> bool {
    text.chars()
        .any(|c| !c.is_whitespace() && !is_punctuation(c))
}

pub fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(
            c,
            '\u{00A1}'..='\u{00BF}'
                | '\u{2010}'..='\u{205E}'
                | '\u{3000}'..='\u{303F}'
                | '\u{FF01}'..='\u{FF0F}'
                | '\u{FF1A}'..='\u{FF20}'
                | '\u{FF3B}'..='\u{FF40}'
                | '\u{FF5B}'..='\u{FF65}'
        )
}

/// Byte offset just after the first sentence end in `text`, including trailing enders and closing quotes.
///
/// CJK enders end a sentence immediately; ASCII enders only when whitespace follows,
/// so that "3.14" or a still-growing "U.S" is not cut.
pub fn first_sentence_end(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let after = index + c.len_utf8();
        let is_end = match c {
            '。' | '！' | '？' | '…' => true,
            '.' | '!' | '?' => chars.peek().is_some_and(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if is_end {
            return Some(after + trailing_closers_len(&text[after..]));
        }
    }
    None
}

fn trailing_closers_len(text: &str) -> usize {
    text.chars()
        .take_while(|c| {
            matches!(
                c,
                '"' | '\''
                    | '”'
                    | '’'
                    | '」'
                    | '』'
                    | ')'
                    | '）'
                    | '。'
                    | '！'
                    | '？'
                    | '…'
                    | '!'
                    | '?'
            )
        })
        .map(char::len_utf8)
        .sum()
}
